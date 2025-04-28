use rsip::param::OtherParam;
use rsip::typed::{Contact, Via};
use rsip::Transport::Tcp;
use rsip::{HostWithPort, Scheme, Uri, Version};
use std::net::SocketAddr;
use uuid::Uuid;


#[derive(Clone)]
pub struct Config {
    /// SIP Server address with port
    pub server_addr: SocketAddr,
    /// Address used to be reached for RTP session, usually the current IP
    pub own_addr: SocketAddr,

    /// SIP Username
    pub username: String,
    /// SIP Password
    pub password: String,

    /// Start of the RTP port range
    pub rtp_port_start: u16,
    /// End of the RTP port range, must be > to `rtp_port_start`
    pub rtp_port_end: u16,
}

impl Config {
    pub fn get_own_uri(&self) -> Uri {
        Uri {
            scheme: Some(Scheme::Sip),
            auth: Some((self.username.clone(), Option::<String>::None).into()),
            host_with_port: HostWithPort::from(self.own_addr),
            ..Default::default()
        }
    }

    pub fn get_own_contact(&self) -> Contact {
        Contact {
            display_name: None,
            uri: self.get_own_uri(),
            params: vec![],
        }
    }

    pub fn get_own_via(&self) -> Via {
        Via {
            version: Version::V2,
            transport: Tcp,
            uri: Uri {
                host_with_port: HostWithPort::from(self.own_addr),
                ..Default::default()
            },
            params: vec![
                rsip::Param::Branch(rsip::param::Branch::new(format!("z9hG4bK{}", Uuid::new_v4()))),
                rsip::Param::Other(OtherParam::new("rport".to_string()), None)
            ],
        }
    }
}