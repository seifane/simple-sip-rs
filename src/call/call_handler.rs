use anyhow::{Result};

use rsip::prelude::*;
use rsip::{Method, Request, Response, SipMessage, StatusCode};
use log::{debug, error, warn};
use crate::call::CallControl;
use crate::call::session_parameters::SessionParameters;
use crate::connection::call_connection::CallConnection;
use crate::utils::BidirectionalChannel;

pub struct CallHandler {
    is_terminated: bool,

    session_params: SessionParameters,

    call_channel: BidirectionalChannel<CallControl>,
    connection: CallConnection,
}

impl CallHandler {
    pub async fn new(
        call_channel: BidirectionalChannel<CallControl>,
        connection: CallConnection,
        session_params: SessionParameters
    ) -> Result<Self>
    {
        Ok(Self {
            is_terminated: false,

            session_params,

            call_channel,
            connection,
        })
    }

    pub fn is_running(&self) -> bool {
        !self.call_channel.one_sided() && !self.is_terminated
    }

    pub async fn handle_next(&mut self) -> Result<()> {
        if self.call_channel.one_sided() {
            debug!("Control channel closed");
            return self.hangup().await;
        }

        tokio::select! {
            call_message = self.call_channel.receiver.recv() => {
                if let Some(message) = call_message {
                    self.handle_call_message(message).await?;
                }
            },
            sip_message = self.connection.recv() => {
                if let Some(message) = sip_message {
                    self.handle_sip_message(message).await?;
                }
            },
        }
        Ok(())
    }

    fn notify_call_hangup(&mut self) {
        let _ = self.call_channel.sender.send(CallControl::Hangup);
        self.is_terminated = true;
    }

    async fn hangup(&mut self) -> Result<()> {
        let mut headers = self.session_params.get_headers_request();
        headers.unique_push(rsip::typed::CSeq::from((self.session_params.get_next_cseq(), Method::Bye)).into());

        let req = Request {
            method: Method::Bye,
            uri: self.session_params.remote.uri.clone(),
            version: Default::default(),
            headers,
            body: Vec::new(),
        };

        self.connection.send_message(req.into()).await?;

        self.notify_call_hangup();
        Ok(())
    }

    async fn handle_sip_message(&mut self, message: SipMessage) -> Result<()>
    {
        match message {
            SipMessage::Request(req) => self.handle_sip_request(req).await,
            SipMessage::Response(res) => self.handle_sip_response(res).await
        }
    }

    async fn handle_sip_response(&mut self, res: Response) -> Result<()>
    {
        if let Ok(cseq) = res.cseq_header() {
            match cseq.method()? {
                _ => {
                    warn!("Unhandled call response {}", cseq);
                }
            }
        }
        Ok(())
    }

    async fn handle_sip_request(&mut self, req: Request) -> Result<()>
    {
        match req.method {
            Method::Bye => self.handle_bye_request(req).await?,
            _ => {
                warn!("Unhandled request {}", req.method)
            }
        }
        Ok(())
    }

    async fn handle_bye_request(&mut self, request: Request) -> Result<()>
    {
        let headers = self.session_params.get_headers_response(&request);
        let response = Response {
            status_code: StatusCode::OK,
            version: Default::default(),
            headers,
            body: vec![],
        };

        let res = self.connection.send_message(response.into()).await;

        self.notify_call_hangup();

        res
    }

    async fn handle_call_message(&mut self, call_control: CallControl) -> Result<()>
    {
        match call_control {
            CallControl::Hangup => self.hangup().await?,
            _ => {}
        }
        Ok(())
    }
}

impl Drop for CallHandler {
    fn drop(&mut self) {
        let _ = self.call_channel.send(CallControl::Finished);
    }
}

pub async fn call_task(
    call_channel: BidirectionalChannel<CallControl>,
    connection: CallConnection,
    session_params: SessionParameters
) -> Result<()> {
    let mut call_handler = CallHandler::new(
        call_channel,
        connection,
        session_params
    ).await?;

    while call_handler.is_running() {
        if let Err(e) = call_handler.handle_next().await {
            error!("call_handler: handle_next error {:#?}", e);
        }
    }

    Ok(())
}