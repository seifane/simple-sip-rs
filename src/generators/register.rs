use anyhow::Result;

use md5::{Digest, Md5};
use rsip::{HostWithPort, Method, Scheme, SipMessage};
use rsip::headers::auth;
use rsip::headers::auth::Algorithm;
use rsip::Method::{Ack, Bye, Cancel, Invite};
use rsip::param::OtherParam;
use rsip::Param::Transport;
use rsip::prelude::*;
use rsip::Transport::Tcp;
use rsip::typed::{Allow, CSeq};
use uuid::Uuid;
use crate::config::Config;

pub struct ConfigAuth<'a> {
    pub config: &'a Config,
    pub realm: String,
    pub nonce: String,
}

fn get_md5(input: String) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

pub fn add_auth_header(mut message: SipMessage, payload: &ConfigAuth) -> Result<SipMessage> {
    let hash1 = get_md5(format!("{}:{}:{}", payload.config.username, payload.realm, payload.config.password));
    let hash2 = get_md5(format!(
        "{}:sip:{};transport=TCP",
        message.cseq_header()?.method()?.to_string(),
        payload.config.server_addr.ip()
    ));
    let auth_response = get_md5(format!("{}:{}:{}", hash1, payload.nonce, hash2));

    let auth_header = rsip::typed::Authorization {
        scheme: auth::Scheme::Digest,
        username: payload.config.username.clone(),
        realm: payload.realm.clone(),
        nonce: payload.nonce.clone(),
        uri: rsip::Uri {
            scheme: Some(Scheme::Sip),
            host_with_port: HostWithPort::from((payload.config.server_addr.ip(), None::<u16>)),
            params: vec![Transport(Tcp)],
            ..Default::default()
        },
        response: auth_response,
        algorithm: Some(Algorithm::Md5),
        opaque: None,
        qop: None,
    };

    message.headers_mut().push(auth_header.into());
    Ok(message)
}

pub fn generate_register_request(config: &Config) -> SipMessage {
    let mut headers: rsip::Headers = Default::default();

    let self_uri = rsip::Uri {
        scheme: Some(Scheme::Sip),
        auth: Some((config.username.clone(), Option::<String>::None).into()),
        host_with_port: HostWithPort::from(config.own_addr),
        ..Default::default()
    };
    let remote_uri = rsip::Uri {
        scheme: Some(Scheme::Sip),
        auth: Some((config.username.clone(), Option::<String>::None).into()),
        host_with_port: HostWithPort::from(config.server_addr),
        params: vec![Transport(Tcp)],
        ..Default::default()
    };


    headers.push(rsip::typed::Via {
        version: rsip::Version::V2,
        transport: Tcp,
        uri: rsip::Uri {
            host_with_port: HostWithPort::from(config.own_addr),
            ..Default::default()
        },
        params: vec![
            rsip::Param::Branch(rsip::param::Branch::new(format!("z9hG4bK{}", Uuid::new_v4()))),
            // rsip::Param::Branch(rsip::param::Branch::new(format!("z9hG4bKasdasdasdasdasdad"))),
            rsip::Param::Other(OtherParam::new("rport".to_string()), None),
        ],
    }.into());
    headers.push(rsip::headers::MaxForwards::default().into());

    headers.push(
        rsip::typed::Contact {
            display_name: None,
            uri: self_uri,
            params: vec![],
        }.into(),
    );
    headers.push(rsip::typed::To {
        display_name: None,
        uri: remote_uri.clone(),
        params: vec![],
    }.into());
    headers.push(rsip::typed::From {
        display_name: None,
        uri: remote_uri.clone(),
        params: vec![rsip::Param::Tag(rsip::param::Tag::new("a73kszlflasda"))],
    }.into());
    headers.push(rsip::headers::CallId::from("fB51qweqqvxBQrvBY2t0_Q..").into()); // TODO: use generated callid
    headers.push(
        CSeq {
            seq: 1,
            method: Method::Register,
        }.into(),
    );

    headers.push(Allow::from(vec![Invite, Ack, Bye, Cancel]).into());
    headers.push(rsip::headers::UserAgent::new("rust-sip").into());
    headers.push(rsip::headers::ContentLength::default().into());

    rsip::Request {
        method: Method::Register,
        uri: rsip::Uri {
            scheme: Some(Scheme::Sip),
            host_with_port: HostWithPort::from((config.server_addr.ip(), None::<u16>)),
            params: vec![Transport(Tcp)],
            ..Default::default()
        },
        version: rsip::Version::V2,
        headers,
        body: Default::default(),
    }.into()
}