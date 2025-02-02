use crate::call::session_parameters::SessionParameters;
use crate::call::Call;
use crate::connection::call_connection::CallConnection;
use crate::context::SipContext;
use anyhow::Result;
use rsip::headers::ContentLength;
use rsip::typed::{Allow, ContentType, MediaType};
use rsip::{Method, Request, Response, StatusCode, Uri, Version};

/// Represents an incoming call.
/// You can choose to either accept or reject the incoming call.
/// Accepting the call will yield a [Call].
/// Rejecting the call will send a `BusyEverywhere` response.
///
/// # Examples
/// ```
///  use simple_sip_rs::call::Call;
///  use simple_sip_rs::call::incoming_call::IncomingCall;
///  let incoming_call: IncomingCall = ...;
///
///  let call: Call = incoming_call.accept().await.unwrap();
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
    /// Sends an OK response to the received invite, initialize and return the [Call]
    ///
    /// # Errors
    ///
    /// The function will return an error if it fails to reply.
    /// This could happen for multiple reasons, for example, the connection was lost to the SIP server.
    ///
    /// The function will return an error if it fails to initialize the Call.
    /// This could happen for multiple reasons, for example, no compatible codecs where found or the response was malformed.
    pub async fn accept(self) -> Result<Call>
    {
        let body = self.call_session_params.local.sdp.to_string().into_bytes();

        let mut headers = self.call_session_params.get_headers_response(&self.request);
        headers.push(Allow::from(vec![Method::Invite, Method::Ack, Method::Bye, Method::Cancel]).into());
        headers.unique_push(ContentType(MediaType::Sdp(Vec::new())).into());
        headers.unique_push(ContentLength::from(body.len() as u32).into());

        let ok_res = Response {
            status_code: StatusCode::OK,
            version: Version::V2,
            headers,
            body,
        };
        self.call_connection.send_message(ok_res.into()).await?;

        Ok(Call::new(self.call_connection, self.call_session_params).await?)
    }

    /// Reject the incoming call.
    /// Send a BusyEverywhere response to the received invite.
    ///
    /// # Errors
    ///
    /// The function will return an error if it fails to reply.
    /// This could happen for multiple reasons, for example, the connection was lost to the SIP server.
    pub async fn reject(self) -> Result<()>
    {
        self.call_connection.send_message(self.generate_busy_response()?.into()).await?;
        Ok(())
    }

    async fn send_ringing(&mut self) -> Result<()> {
        self.call_connection.send_message(self.generate_ringing_response()?.into()).await?;
        Ok(())
    }

    fn generate_busy_response(&self) -> Result<Response> {
        let mut headers = self.call_session_params.get_headers_response(&self.request);
        headers.push(Allow::from(vec![Method::Invite, Method::Ack, Method::Bye, Method::Cancel]).into());

        let busy_response = Response {
            status_code: StatusCode::BusyEverywhere,
            version: Version::V2,
            headers,
            body: Default::default(),
        };

        Ok(busy_response)
    }

    fn generate_ringing_response(&self) -> Result<Response> {
        let mut headers = self.call_session_params.get_headers_response(&self.request);
        headers.push(Allow::from(vec![Method::Invite, Method::Ack, Method::Bye, Method::Cancel]).into());

        let ringing_response = Response {
            status_code: StatusCode::Ringing,
            version: Version::V2,
            headers: headers.clone(),
            body: Default::default(),
        };

        Ok(ringing_response)
    }
}