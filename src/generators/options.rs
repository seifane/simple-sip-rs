use rsip::{HostWithPort, Request, Scheme, SipMessage, StatusCode};
use rsip::headers::AcceptLanguage;
use rsip::Method::{Ack, Bye, Cancel, Invite};
use rsip::prelude::*;
use rsip::typed::{Accept, Allow, MediaType};
use crate::config::Config;

pub fn generate_options_response(request: Request, config: &Config) -> SipMessage {
    let mut headers: rsip::Headers = Default::default();

    let request_via = request.via_header().unwrap().clone().into_typed().unwrap();
    headers.push(request_via.into());

    headers.push(
        rsip::typed::Contact {
            display_name: None,
            uri: rsip::Uri {
                scheme: Some(Scheme::Sip),
                auth: Some((config.username.clone(), Option::<String>::None).into()),
                host_with_port: HostWithPort::from(config.own_addr),
                ..Default::default()
            },
            params: vec![],
        }.into(),
    );
    headers.push(request.to_header().unwrap().clone().into());
    headers.push(request.from_header().unwrap().clone().into());
    headers.push(request.call_id_header().unwrap().clone().into());
    headers.push(request.cseq_header().unwrap().clone().into());

    headers.push(Allow::from(vec![Invite, Ack, Bye, Cancel]).into());
    headers.push(Accept::from(vec![MediaType::Sdp(Default::default())]).into());
    headers.push(AcceptLanguage::from("en").into());

    headers.push(rsip::headers::UserAgent::new("rust-sip").into());
    headers.push(rsip::headers::ContentLength::default().into());

    rsip::Response {
        status_code: StatusCode::OK,
        version: rsip::Version::V2,
        headers,
        body: Default::default(),
    }.into()
}