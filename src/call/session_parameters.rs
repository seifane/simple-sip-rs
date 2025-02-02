use anyhow::{Context, Result};
use rsip::headers::{ContentLength, MaxForwards};
use rsip::param::Tag;
use rsip::prelude::*;
use rsip::{Header, Headers, Request, Response, Uri};
use uuid::Uuid;
use webrtc_sdp::{parse_sdp, SdpSession};

use crate::config::Config;
use crate::context::SipContext;
use crate::generators::sdp::generate_sdp_new;

#[derive(Clone)]
pub struct LocalSessionParameters {
    pub uri: Uri,
    pub tag: String,
    pub sdp: SdpSession,
    pub port: u16,
}

#[derive(Clone)]
pub struct RemoteSessionParameters {
    pub uri: Uri,
    pub tag: String,
    pub sdp: SdpSession,
}

#[derive(Clone)]
pub struct SessionParameters
{
    cseq: u32,
    pub call_id: String,

    pub remote: RemoteSessionParameters,
    pub local: LocalSessionParameters,

    pub config: Config,
}

impl SessionParameters {
    pub fn from_request(context: &mut SipContext, request: &Request) -> Result<Self> {
        let from = request.headers.iter().find_map(|i| {
            if let Header::From(from) = i {
                let typed = from.clone().into_typed().unwrap();
                return Some(typed.clone())
            }
            None
        }).context("Remote uri not found")?;
        let call_id = request.call_id_header()?.value().to_string();

        let body = String::from_utf8(request.body().clone())?;
        let remote_uri = from.uri.clone();
        let remote_sdp = parse_sdp(body.as_str(), false)?;
        let remote_tag = from.tag().context("Remote tag not found")?.value().to_string();

        let local_port = context.get_next_udp_port();

        Ok(Self {
            cseq: request.cseq_header()?.seq()?,
            call_id,

            remote: RemoteSessionParameters {
                uri: remote_uri,
                tag: remote_tag,
                sdp: remote_sdp,
            },
            local: LocalSessionParameters {
                uri: context.config.get_own_uri(),
                tag: format!("tt{}", Uuid::new_v4()),
                sdp: generate_sdp_new(&context.config, local_port)?,
                port: local_port,
            },

            config: context.config.clone(),
        })
    }

    pub fn from_response(
        response: &Response,
        call_id: String,
        local: LocalSessionParameters,
        config: Config
    ) -> Result<Self> {
        let to = response.headers.iter().find_map(|i| {
            if let Header::To(from) = i {
                let typed = from.clone().into_typed().unwrap();
                return Some(typed.clone())
            }
            None
        }).context("Remote uri not found")?;
        let remote_tag = to.tag().context("To tag not found")?.value().to_string();

        let body = String::from_utf8(response.body().clone())?;
        let remote_sdp = parse_sdp(body.as_str(), false)?;

        let cseq = response.cseq_header()?.seq()?;

        Ok(Self {
            cseq,
            call_id,
            remote: RemoteSessionParameters {
                uri: to.uri,
                tag: remote_tag,
                sdp: remote_sdp,
            },
            local,
            config,
        })
    }

    pub fn get_headers_request(&self) -> Headers
    {
        let mut params = Vec::new();
        params.push(rsip::Param::Tag(Tag::new(&self.remote.tag)));

        let headers: Vec<Header> = vec![
            self.config.get_own_via().into(),
            MaxForwards::default().into(),
            rsip::headers::CallId::from(self.call_id.clone()).into(),
            self.config.get_own_contact().into(),
            rsip::typed::From {
                display_name: None,
                uri: self.local.uri.clone(),
                params: vec![
                    rsip::Param::Tag(Tag::new(&self.local.tag)),
                ],
            }.into(),
            rsip::typed::To {
                display_name: None,
                uri: self.remote.uri.clone(),
                params,
            }.into(),
            ContentLength::default().into(),
            rsip::headers::UserAgent::new("sip-rs").into()
        ];

        rsip::Headers::from(headers)
    }

    pub fn get_headers_response(&self, request: &Request) -> Headers
    {
        let mut params = Vec::new();
        params.push(rsip::Param::Tag(Tag::new(&self.remote.tag)));

        let headers: Vec<Header> = vec![
            MaxForwards::default().into(),
            request.via_header().unwrap().clone().into(),
            rsip::headers::CallId::from(self.call_id.clone()).into(),
            rsip::typed::From {
                display_name: None,
                uri: self.remote.uri.clone(),
                params,
            }.into(),
            rsip::typed::To {
                display_name: None,
                uri: self.local.uri.clone(),
                params: vec![
                    rsip::Param::Tag(Tag::new(&self.local.tag)),
                ],
            }.into(),
            request.cseq_header().unwrap().typed().unwrap().into(),
            ContentLength::default().into(),
            rsip::headers::UserAgent::new("sip-rs").into()
        ];

        rsip::Headers::from(headers)
    }

    pub fn get_next_cseq(&mut self) -> u32 {
        self.cseq += 1;
        self.cseq
    }
}