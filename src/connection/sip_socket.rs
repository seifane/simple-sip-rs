use crate::call::incoming_call::IncomingCall;
use crate::connection::call_connection::CallConnection;
use crate::context::SipContext;
use crate::sip_proto::options::generate_options_response;
use crate::sip_proto::register::{add_auth_header, generate_register_request, ConfigAuth};
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use rsip::headers::ToTypedHeader;
use rsip::prelude::{HeadersExt, UntypedHeader};
use rsip::{Method, Request, SipMessage, StatusCode};
use std::ops::DerefMut;
use std::sync::Arc;
use futures_util::StreamExt;
use tokio::io::{AsyncWriteExt};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::Mutex;
use tokio_util::codec::FramedRead;
use crate::connection::socket_data::SocketData;
use crate::sip_proto::sip_message_decoder::SipMessageDecoder;

pub struct SipSocket {
    sip_message_reader: FramedRead<OwnedReadHalf, SipMessageDecoder>,
    stream_write: OwnedWriteHalf,

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
        let (stream_read, stream_write) = stream.into_split();
        let (sender, receiver) = channel(64);

        let mut instance = Self {
            sip_message_reader: FramedRead::new(stream_read, SipMessageDecoder::new()),

            stream_write,
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
        loop {
            tokio::select! {
                read = self.sip_message_reader.next() => {
                    if let Some(message) = read {
                        match message {
                            Ok(message) => {
                                if self.handle_call_message(&message).await {
                                    continue;
                                }
                                self.handle_message(message).await?;
                            }
                            Err(e) => {
                                error!("SIP message read error: {:?}", e);
                            }
                        }
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
        self.stream_write
            .write_all(message.to_string().as_bytes())
            .await?;
        Ok(())
    }

    async fn read_next_message(&mut self) -> Result<SipMessage> {
        loop {
            if let Some(message) = self.sip_message_reader.next().await {
                return Ok(message?)
            }
        }
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
