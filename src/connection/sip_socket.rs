use crate::call::incoming_call::IncomingCall;
use crate::connection::call_connection::CallConnection;
use crate::context::SipContext;
use crate::generators::options::generate_options_response;
use crate::generators::register::{add_auth_header, generate_register_request, ConfigAuth};
use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use rsip::headers::ToTypedHeader;
use rsip::prelude::{HasHeaders, HeadersExt, UntypedHeader};
use rsip::Header::ContentLength;
use rsip::{Method, Request, SipMessage, StatusCode};
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Mutex;
use crate::connection::socket_data::SocketData;

fn get_next_packet_split(buffer: &Vec<u8>) -> Option<usize> {
    buffer
        .windows(4)
        .enumerate()
        .find(|&(_, w)| matches!(w, b"\r\n\r\n"))
        .map(|(ix, _)| ix + 4)
}

fn try_parse_sip_header_from_buffer(buffer: &mut Vec<u8>) -> Result<Option<SipMessage>> {
    let index = get_next_packet_split(&buffer);
    if let Some(index) = index {
        let packet = buffer.drain(..index).collect::<Vec<_>>();
        if packet.len() == 4 {
            // TODO: check content as well
            debug!("Received keep alive");
            return Ok(None);
        }

        return Ok(Some(SipMessage::try_from(packet.as_slice())?));
    }
    Ok(None)
}

fn get_message_content_length(sip_message: &SipMessage) -> usize {
    sip_message
        .headers()
        .iter()
        .find_map(|header| {
            if let ContentLength(header) = header {
                Some(header.length().unwrap_or(0))
            } else {
                None
            }
        })
        .unwrap_or(0) as usize
}

fn try_parse_body(sip_message: &mut SipMessage, buffer: &mut Vec<u8>) -> Result<()> {
    let content_length = get_message_content_length(sip_message);
    if content_length > 0 {
        if buffer.len() < content_length as usize {
            return Err(anyhow!("Buffer not filled enough"));
        }
        let mut body = buffer.drain(..content_length as usize).collect();
        sip_message.body_mut().append(&mut body);
    }
    Ok(())
}

pub struct SipSocket {
    buffer: Vec<u8>,

    stream: TcpStream,
    message_receiver: Receiver<SipMessage>,
    message_sender: Sender<SipMessage>,
    incoming_call_sender: Sender<IncomingCall>,

    sip_context: Arc<Mutex<SipContext>>,
    socket_data: Arc<Mutex<SocketData>>,
}

impl SipSocket {
    pub async fn connect<A: ToSocketAddrs>(
        addr: A,
        sip_context: Arc<Mutex<SipContext>>,
        incoming_call_sender: Sender<IncomingCall>,
    ) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (sender, receiver) = channel(64);

        let mut instance = Self {
            buffer: Vec::new(),

            stream,
            message_sender: sender,
            message_receiver: receiver,
            incoming_call_sender,

            sip_context,
            socket_data: Arc::new(Mutex::new(SocketData::default())),
        };

        instance.register().await?;
        Ok(instance)
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut read_buffer = [0; 4096];

        loop {
            tokio::select! {
                read = self.stream.read(&mut read_buffer) => {
                    let read = read?;
                    self.buffer.extend_from_slice(&read_buffer[..read]);
                    let message = self.get_next_message().await?;
                    if let Some(message) = message {
                        if self.handle_call_message(&message).await {
                            continue;
                        }
                        self.handle_message(message).await?;
                    }
                }
                message = self.message_receiver.recv() => {
                    match message {
                        None => return Ok(()),
                        Some(message) => self.send_message(message).await?,
                    }
                }
            }
        }
    }

    pub(crate) fn get_socket_data(&self) -> Arc<Mutex<SocketData>> {
        self.socket_data.clone()
    }

    pub(crate) fn get_message_sender(&self) -> Sender<SipMessage> {
        self.message_sender.clone()
    }

    async fn register(&mut self) -> Result<()> {
        info!("Registering SIP");

        let config = self.sip_context.lock().await.config.clone();

        let req = generate_register_request(&config);
        self.send_message(req.clone().into()).await?;
        info!("Sent SIP REGISTER request");

        let response = self.read_next_message().await?;
        info!("Received SIP REGISTER response");

        if let SipMessage::Response(response) = response {
            match response.status_code {
                StatusCode::Unauthorized => {
                    let www_authenticate_header = response
                        .www_authenticate_header()
                        .unwrap()
                        .clone()
                        .into_typed()?;

                    let register_auth_payload = ConfigAuth {
                        config: &config,
                        realm: www_authenticate_header.realm,
                        nonce: www_authenticate_header.nonce,
                    };

                    let mut req = add_auth_header(req, &register_auth_payload)?;
                    req.cseq_header_mut()?.mut_seq(2)?;

                    self.send_message(req.into()).await?;
                    let response = self.read_next_message().await?;

                    if let SipMessage::Response(response) = response {
                        if response.status_code == StatusCode::OK {
                            info!("Successfully registered");
                            return Ok(());
                        }
                        return Err(anyhow!(
                            "Failed to register with status code: {}",
                            response.status_code
                        ));
                    }

                    Err(anyhow!("Did not get expected response"))
                }
                StatusCode::OK => {
                    info!("Successfully registered");
                    Ok(())
                }
                _ => Err(anyhow!(
                    "Got unexpected status code {}",
                    response.status_code
                )),
            }
        } else {
            Err(anyhow!("Did not get expected response"))
        }
    }

    async fn send_message(&mut self, message: SipMessage) -> Result<()> {
        self.stream
            .write_all(message.to_string().as_bytes())
            .await?;
        Ok(())
    }

    async fn read_next_message(&mut self) -> Result<SipMessage> {
        loop {
            let mut read_buffer = [0; 4096];
            let read = self.stream.read(&mut read_buffer).await?;
            self.buffer.extend_from_slice(&read_buffer[..read]);

            let message = self.get_next_message().await?;
            if let Some(message) = message {
                return Ok(message);
            }
        }
    }

    async fn get_next_message(&mut self) -> Result<Option<SipMessage>> {
        let message = try_parse_sip_header_from_buffer(&mut self.buffer)?;
        if let Some(mut message) = message {
            let content_length = get_message_content_length(&message);
            if content_length > 0 && content_length < self.buffer.len() {
                self.ensure_buffer_full(content_length).await?;
            }
            try_parse_body(&mut message, &mut self.buffer)?;
            return Ok(Some(message));
        }
        Ok(None)
    }

    async fn ensure_buffer_full(&mut self, len: usize) -> Result<()> {
        if self.buffer.len() < len {
            let mut l_buffer = vec![0; len - self.buffer.len()];
            let n = self.stream.read_exact(&mut l_buffer).await?;
            self.buffer.extend_from_slice(&l_buffer[..n]);
        }
        Ok(())
    }

    async fn handle_message(&mut self, message: SipMessage) -> Result<()> {
        match message {
            SipMessage::Request(request) => self.handle_sip_request(request).await?,
            SipMessage::Response(response) => {
                warn!("Ignored SIP response {:?}", response);
            }
        }
        Ok(())
    }

    async fn handle_sip_request(&mut self, request: Request) -> Result<()> {
        match request.method {
            Method::Options => {
                let response =
                    generate_options_response(request, &self.sip_context.lock().await.config);
                self.send_message(response).await?;
            }
            Method::Invite => {
                let call_id = request.call_id_header()?.value().to_string();
                let call_connection = CallConnection::new(
                    self.message_sender.clone(),
                    self.socket_data
                        .lock()
                        .await
                        .create_call_channel(call_id)
                        .await?,
                );
                let call = IncomingCall::try_from_request(
                    self.sip_context.lock().await.deref_mut(),
                    request,
                    call_connection,
                )
                .await?;
                self.incoming_call_sender.send(call).await?;
            }
            _ => {
                warn!("Ignoring not handled method: {}", request.method);
            }
        }
        Ok(())
    }

    async fn handle_call_message(&mut self, message: &SipMessage) -> bool {
        if let Ok(call_id) = message.call_id_header() {
            let id = call_id.value().to_string();
            let mut socket_data = self.socket_data.lock().await;

            if let Some(channel) = socket_data.call_channels.get_mut(&id) {
                if channel.send(message.clone()).await.is_err() {
                    warn!("Sent to call channel failed, dropping");
                    socket_data.call_channels.remove(&id);
                }
                return true;
            }
        }
        false
    }
}
