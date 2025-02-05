use anyhow::{anyhow, Result};
use log::{debug, info};
use crate::call::session_parameters::{SessionParameters, LocalSessionParameters};
use crate::call::Call;
use crate::config::Config;
use crate::connection::call_connection::CallConnection;
use crate::context::SipContext;
use crate::sip_proto::sdp::generate_sdp_new;
use rsip::headers::{ContentLength, MaxForwards, ToTypedHeader};
use rsip::param::Tag;
use rsip::prelude::{HeadersExt, UntypedHeader};
use rsip::typed::{CSeq, ContentType, MediaType, Via};
use rsip::{Headers, Method, Param, Request, Response, SipMessage, StatusCode, Uri};
use uuid::Uuid;
use crate::sip_proto::register::{add_auth_header, ConfigAuth};

pub enum OutgoingCallResponse {
    Accepted(Call),
    Rejected(StatusCode)
}

pub enum PeekOutgoingCallResponse {
    Accepted,
    Rejected(StatusCode),
}

/// Represents an outgoing call that has yet to start.
/// To progress to the call see [into_call_response](OutgoingCall::into_call_response).
///
/// # Examples
/// ```
///  use simple_sip_rs::call::outgoing_call::{OutgoingCall, OutgoingCallResponse};
///
///  async fn handle_outgoing_call(outgoing_call: OutgoingCall) {
///     let response = outgoing_call.into_call_response().await.unwrap();
///     match response {
///         OutgoingCallResponse::Accepted(call) => {
///         // ...
///         call.hangup().unwrap();
///         }
///         OutgoingCallResponse::Rejected(status_code) => {
///             println!("Call was rejected with status code {status_code}");
///         }
///     }
///  }
/// ```
pub struct OutgoingCall {
    call_connection: CallConnection,

    call_id: String,
    remote_uri: Uri,
    cseq: u32,
    own_via: Via,

    local_call_session_params: LocalSessionParameters,
    config: Config,

    response: Option<Response>
}

impl OutgoingCall {
    pub(crate) async fn try_from(
        sip_context: &mut SipContext,
        call_connection: CallConnection,
        call_id: String,
        uri: Uri
    ) -> Result<Self>
    {
        let local_port = sip_context.get_next_udp_port();

        let local_call_session_params = LocalSessionParameters {
            uri: sip_context.config.get_own_uri(),
            tag: format!("tt{}", Uuid::new_v4()),
            sdp: generate_sdp_new(&sip_context.config, local_port)?,
            port: local_port,
        };


        let mut instance = OutgoingCall {
            call_connection,

            call_id,
            remote_uri: uri,
            cseq: 1234,
            own_via: sip_context.config.get_own_via(),

            local_call_session_params,
            config: sip_context.config.clone(),

            response: None
        };
        instance.send_invite().await?;
        Ok(instance)
    }

    /// Listens and blocks for a response to the call without consuming the [OutgoingCall].
    ///
    /// This is useful if you are not sure if you want to proceed with the call yet but still want to listen for responses.
    /// For example to [cancel](OutgoingCall::cancel) the call after a timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use simple_sip_rs::call::outgoing_call::OutgoingCall;
    ///
    ///  async fn handle_outgoing_call(mut outgoing_call: OutgoingCall) {
    ///     if let Err(_) = tokio::time::timeout(Duration::from_secs(10), outgoing_call.peek_call_response())
    ///     {
    ///         //Future has timed out after 10 seconds, we cancel the call.
    ///         outgoing_call.cancel().unwrap();
    ///     }
    ///
    ///  }
    /// ```
    pub async fn peek_call_response(&mut self) -> Result<PeekOutgoingCallResponse>
    {
        loop {
            if let Some(message) = self.call_connection.recv().await {
                match message {
                    SipMessage::Request(r) => info!("Ignored request while waiting for answer: {:?}", r),
                    SipMessage::Response(response) => {
                        self.handle_response(response).await?;
                        if let Some(response) = self.response.as_ref() {
                            if response.status_code == StatusCode::OK {
                                return Ok(PeekOutgoingCallResponse::Accepted);
                            } else {
                                return Ok(PeekOutgoingCallResponse::Rejected(response.status_code.clone()));
                            }
                        }
                    }
                }
            } else {
                return Err(anyhow!("Call connection closed unexpectedly"));
            }
        }
    }


    /// Consumes the [OutgoingCall] into an [OutgoingCallResponse]. This function will block until a response is received.
    ///
    /// If the call is accepted, returns [OutgoingCallResponse::Accepted] containing the [Call].
    ///
    /// If the call is rejected, returns [OutgoingCallResponse::Rejected] containing the received [StatusCode].
    ///
    /// # Errors
    ///
    /// This function will return an error in the following cases :
    /// - Failed to acknowledge the Ok response on the invite
    /// - Received a response that was not related to the invite
    /// - The received response was malformed
    /// - Connection to the SIP server was lost
    pub async fn into_call_response(mut self) -> Result<OutgoingCallResponse> {
        if let Some(response) = self.response.take() {
            return Ok(self.get_outgoing_call_response(response).await?);
        }

        self.peek_call_response().await?;

        if let Some(response) = self.response.take() {
            return Ok(self.get_outgoing_call_response(response).await?);
        }
        Err(anyhow!("Unable to get call from outgoing call"))
    }


    /// Cancel the invite (hangup before answer)
    ///
    /// This will cancel the outgoing call and consume it. The remote phone will stop ringing.
    ///
    /// # Errors
    ///
    /// This function will return an error if the sending of the message fails,
    /// most likely because the underlying connection was closed.
    ///
    /// # Examples
    ///
    /// See combined usage example with [peek_call_response](OutgoingCall::peek_call_response)
    pub async fn cancel(mut self) -> Result<()> {
        let request = self.generate_cancel();
        self.call_connection.send_message(request.into()).await?;
        Ok(())
    }

    async fn handle_response(&mut self, response: Response) -> Result<()>
    {
        if response.cseq_header()?.method()? != Method::Invite {
            return Err(anyhow!("Unexpected response while waiting for answer: {:?}", response));
        }
        match response.status_code {
            StatusCode::Trying => info!("Remote is trying"),
            StatusCode::Ringing => info!("Remote is ringing"),
            StatusCode::BusyHere |
            StatusCode::BusyEverywhere |
            StatusCode::ServiceUnavailable |
            StatusCode::TemporarilyUnavailable |
            StatusCode::OK => {
                self.response = Some(response);
            }
            StatusCode::SessionProgress => {
                debug!("Explicit ignore {:?}", response);
            }
            StatusCode::Unauthorized => self.handle_invite_response_unauthorized(response).await?,
            _ => {
                info!("Unexpected response while waiting for invite: {:?}", response);
            }
        };
        Ok(())
    }

    async fn get_outgoing_call_response(self, response: Response) -> Result<OutgoingCallResponse> {
        if response.status_code == StatusCode::OK {
            let session_params = SessionParameters::from_response(
                &response,
                self.call_id.clone(),
                self.local_call_session_params.clone(),
                self.config.clone()
            )?;

            let mut headers = session_params.get_headers_request();
            headers.unique_push(rsip::typed::CSeq::from((response.cseq_header()?.seq()?, Method::Ack)).into());

            let response = Request {
                method: Method::Ack,
                uri: session_params.remote.uri.clone(),
                version: Default::default(),
                headers,
                body: vec![],
            };

            self.call_connection.send_message(response.into()).await?;

            return Ok(OutgoingCallResponse::Accepted(Call::new(self.call_connection, session_params).await?));
        }
        Ok(OutgoingCallResponse::Rejected(response.status_code))
    }

    async fn handle_invite_response_unauthorized(&mut self, response: Response) -> Result<()>
    {
        let www_authenticate_header = response.www_authenticate_header()
            .ok_or(anyhow!("Missing authenticate header"))?
            .clone()
            .into_typed()?;

        self.cseq = self.cseq + 1;
        let message = add_auth_header(self.generate_invite().into(), &ConfigAuth {
            config: &self.config,
            realm: www_authenticate_header.realm.clone(),
            nonce: www_authenticate_header.nonce.clone()
        })?;

        self.call_connection.send_message(message).await?;
        Ok(())
    }


    async fn send_invite(&mut self) -> Result<()>
    {
        let request = self.generate_invite();
        self.call_connection.send_message(request.into()).await?;
        Ok(())
    }

    fn generate_invite(&mut self) -> Request
    {
        let body = self.local_call_session_params.sdp.to_string().into_bytes();

        let mut headers = self.get_base_headers();
        headers.unique_push(ContentLength::from(body.len() as u32).into());
        headers.unique_push(ContentType(MediaType::Sdp(Vec::new())).into());
        headers.unique_push(CSeq::from((self.cseq, Method::Invite)).into());
        headers.unique_push(self.config.get_own_contact().into());

        Request {
            method: Method::Invite,
            uri: self.remote_uri.clone(),
            version: Default::default(),
            headers,
            body,
        }
    }

    fn generate_cancel(&mut self) -> Request
    {
        let mut headers = self.get_base_headers();
        headers.unique_push(CSeq::from((self.cseq, Method::Cancel)).into());
        headers.unique_push(ContentLength::from(0).into());

        Request {
            method: Method::Cancel,
            uri: self.remote_uri.clone(),
            version: Default::default(),
            headers,
            body: vec![],
        }
    }

    fn get_base_headers(&self) -> Headers {
        Headers::from(vec![
            MaxForwards::default().into(),
            self.own_via.clone().into(),
            rsip::headers::CallId::from(self.call_id.clone()).into(),
            rsip::typed::From {
                display_name: None,
                uri: self.local_call_session_params.uri.clone(),
                params: vec![
                    Param::Tag(Tag::new(&self.local_call_session_params.tag)),
                ],
            }.into(),
            rsip::typed::To {
                display_name: None,
                uri: self.remote_uri.clone(),
                params: Default::default(),
            }.into(),
            rsip::headers::UserAgent::new("sip-rs").into()
        ])
    }
}