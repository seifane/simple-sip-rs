use anyhow::{anyhow, Result};
use log::{debug, info};
use crate::call::session_parameters::{SessionParameters, LocalSessionParameters};
use crate::call::Call;
use crate::config::Config;
use crate::connection::call_connection::CallConnection;
use crate::context::SipContext;
use crate::generators::sdp::generate_sdp_new;
use rsip::headers::{ContentLength, MaxForwards, ToTypedHeader};
use rsip::param::Tag;
use rsip::prelude::{HeadersExt, UntypedHeader};
use rsip::typed::{CSeq, ContentType, MediaType};
use rsip::{Headers, Method, Param, Request, Response, SipMessage, StatusCode, Uri};
use uuid::Uuid;
use crate::generators::register::{add_auth_header, ConfigAuth};

pub enum OutgoingCallResponse {
    Accepted(Call),
    Rejected(StatusCode)
}

/// Represents an outgoing call that has yet to start.
/// To progress to the call see [wait_for_answer](OutgoingCall::wait_for_answer).
///
/// # Examples
/// ```
///  use simple_sip_rs::call::outgoing_call::{OutgoingCall, OutgoingCallResponse};
///
///  let incoming_call: OutgoingCall = ...;
///
///  let response: OutgoingCallResponse = incoming_call.wait_for_answer().await.unwrap();
///  match response {
///     OutgoingCallResponse::Accepted(call) => {
///         ...
///         call.hangup().unwrap()
///     }
///     OutgoingCallResponse::Rejected(status_code) => {
///         println!("Call was rejected with status code {status_code}")
///     }
///  }
/// ```
pub struct OutgoingCall {
    call_connection: CallConnection,

    call_id: String,
    remote_uri: Uri,
    cseq: u32,

    local_call_session_params: LocalSessionParameters,
    config: Config,
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

            local_call_session_params,
            config: sip_context.config.clone(),
        };
        instance.send_invite().await?;
        Ok(instance)
    }

    /// Will wait for a response of the remote.
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
    pub async fn wait_for_answer(mut self) -> Result<OutgoingCallResponse> {
        loop {
            if let Some(message) = self.call_connection.recv().await {
                match message {
                    SipMessage::Request(r) => info!("Ignored request while waiting for answer: {:?}", r),
                    SipMessage::Response(response) => {
                        if response.cseq_header()?.method()? != Method::Invite {
                            return Err(anyhow!("Unexpected response while waiting for answer: {:?}", response));
                        }
                        if response.status_code == StatusCode::OK {
                            return Ok(OutgoingCallResponse::Accepted(
                                self.handle_invite_response_ok(response).await?
                            ))
                        }
                        if let Some(res) = self.handle_invite_response(response).await? {
                            return Ok(res);
                        }
                    }
                }
            } else {
                return Err(anyhow!("Call connection closed unexpectedly"));
            }
        }
    }

    async fn handle_invite_response(&mut self, response: Response) -> Result<Option<OutgoingCallResponse>> {
        match response.status_code {
            StatusCode::Trying => info!("Remote is trying"),
            StatusCode::Ringing => info!("Remote is ringing"),
            StatusCode::BusyHere | StatusCode::BusyEverywhere | StatusCode::ServiceUnavailable | StatusCode::TemporarilyUnavailable => {
                info!("Remote returned busy");
                return Ok(Some(OutgoingCallResponse::Rejected(response.status_code)));
            }
            StatusCode::OK => {
                // Handled separately for now
            },
            StatusCode::SessionProgress => {
                // Simply ignored for now
                debug!("Explicit ignore {:?}", response);
            }
            StatusCode::Unauthorized => self.handle_invite_response_unauthorized(response).await?,
            _ => {
                info!("Unexpected response while waiting for invite: {:?}", response);
            }
        };
        Ok(None)
    }

    async fn handle_invite_response_ok(self, response: Response) -> Result<Call> {
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

        Ok(Call::new(self.call_connection, session_params).await?)
    }

    async fn handle_invite_response_unauthorized(&mut self, response: Response) -> Result<()>
    {
        self.cseq = self.cseq + 1;
        let request = self.generate_invite()?;

        let www_authenticate_header = response.www_authenticate_header()
            .ok_or(anyhow!("Missing authenticate header"))?
            .clone()
            .into_typed()?;
        let message = add_auth_header(request.into(), &ConfigAuth {
            config: &self.config,
            realm: www_authenticate_header.realm.clone(),
            nonce: www_authenticate_header.nonce.clone()
        })?;

        self.call_connection.send_message(message).await?;
        Ok(())
    }


    async fn send_invite(&mut self) -> Result<()>
    {
        let request = self.generate_invite()?;
        self.call_connection.send_message(request.into()).await?;
        Ok(())
    }

    fn generate_invite(&mut self) -> Result<Request>
    {
        let body = self.local_call_session_params.sdp.to_string().into_bytes();

        let headers = Headers::from(vec![
            MaxForwards::default().into(),
            CSeq::from((self.cseq, Method::Invite)).into(),
            ContentLength::from(body.len() as u32).into(),
            ContentType(MediaType::Sdp(Vec::new())).into(),
            self.config.get_own_via().into(),
            self.config.get_own_contact().into(),
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
        ]);

        Ok(Request {
            method: Method::Invite,
            uri: self.remote_uri.clone(),
            version: Default::default(),
            headers,
            body,
        })
    }
}