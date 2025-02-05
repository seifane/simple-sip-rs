use crate::call::session_parameters::SessionParameters;
use crate::call::Call;
use crate::connection::call_connection::CallConnection;
use crate::context::SipContext;
use anyhow::Result;
use log::info;
use rsip::headers::ContentLength;
use rsip::typed::{ContentType, MediaType};
use rsip::{Method, Request, Response, SipMessage, StatusCode, Uri, Version};

pub enum IncomingCallResult {
    Ok(Call),
    Cancelled,
}

/// Represents an incoming call.
/// You can choose to either accept or reject the incoming call.
/// Accepting the call will yield a [Call].
/// Rejecting the call will send a `BusyEverywhere` response.
///
/// # Examples
/// ```
///  use simple_sip_rs::call::Call;
///  use simple_sip_rs::call::incoming_call::{IncomingCall, IncomingCallResult};
///  async fn handle_incoming_call(incoming_call: IncomingCall)
///  {
///     match incoming_call.accept().await.unwrap() {
///         IncomingCallResult::Ok(call) => {
///             // Do something with the call
///             call.hangup().unwrap()
///         },
///         IncomingCallResult::Cancelled => {
///             // Call was dropped before we could answer it
///         }
///     }
///  }
/// ```
pub struct IncomingCall {
    call_connection: CallConnection,
    call_session_params: SessionParameters,
    request: Request,
}

impl IncomingCall {
    pub(crate) async fn try_from_request(
        context: &mut SipContext,
        request: Request,
        call_connection: CallConnection
    ) -> Result<IncomingCall> {
        let mut instance = Self {
            call_connection,
            call_session_params: SessionParameters::from_request(context, &request)?,
            request,
        };

        instance.send_ringing().await?;
        Ok(instance)
    }

    /// [Uri] of the caller.
    pub fn get_remote_uri(&self) -> &Uri {
        &self.call_session_params.remote.uri
    }

    /// Accept the incoming call.
    ///
    /// - If the call can start: initializes the call and returns [IncomingCallResult::Ok]
    ///
    /// - If the call was cancelled by the remote (already hung up): acknowledges the cancellation and
    ///    returns [IncomingCallResult::Cancelled]
    ///
    /// # Errors
    ///
    /// The function will return an error if it fails to reply.
    /// This could happen for multiple reasons, for example, the connection was lost to the SIP server.
    ///
    /// The function will return an error if it fails to initialize the Call.
    /// This could happen for multiple reasons, for example, no compatible codecs where found or the response was malformed.
    pub async fn accept(mut self) -> Result<IncomingCallResult>
    {
        if let Some(request) = self.get_cancel_request() {
            info!("Trying to accept call but was cancelled");
            let response = self.generate_response(&request, StatusCode::OK);
            self.call_connection.send_message(response.into()).await?;
            return Ok(IncomingCallResult::Cancelled);
        }

        let mut response = self.generate_response(&self.request, StatusCode::OK);

        let body = self.call_session_params.local.sdp.to_string().into_bytes();
        response.headers.unique_push(ContentType(MediaType::Sdp(Vec::new())).into());
        response.headers.unique_push(ContentLength::from(body.len() as u32).into());
        response.body = body;

        self.call_connection.send_message(response.into()).await?;

        Ok(IncomingCallResult::Ok(Call::new(self.call_connection, self.call_session_params).await?))
    }

    /// Reject the incoming call.
    /// Send a BusyEverywhere response to the received invite.
    ///
    /// # Errors
    ///
    /// The function will return an error if it fails to reply.
    /// This could happen for multiple reasons, for example, the connection was lost to the SIP server.
    pub async fn reject(mut self) -> Result<()>
    {
        if let Some(request) = self.get_cancel_request() {
            info!("Try to reject call but was already cancelled");
            let response = self.generate_response(&request, StatusCode::OK);
            self.call_connection.send_message(response.into()).await?;
            return Ok(());
        }
        self.call_connection.send_message(self.generate_response(&self.request, StatusCode::BusyEverywhere).into()).await?;
        Ok(())
    }

    async fn send_ringing(&mut self) -> Result<()> {
        self.call_connection.send_message(self.generate_response(&self.request, StatusCode::Ringing).into()).await?;
        Ok(())
    }

    fn get_cancel_request(&mut self) -> Option<Request> {
        while let Ok(Some(message)) = self.call_connection.try_recv() {
            if let SipMessage::Request(request) = message {
                if request.method == Method::Cancel {
                    return Some(request);
                }
            }
        }
        None
    }

    fn generate_response(&self, request: &Request, status_code: StatusCode) -> Response {
        let ok_res = Response {
            status_code,
            version: Version::V2,
            headers: self.call_session_params.get_headers_response(&request),
            body: Default::default(),
        };
        ok_res
    }

}