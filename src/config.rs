use rsip::param::OtherParam;
use rsip::typed::{Contact, Via};
use rsip::Transport::Tcp;
use rsip::{HostWithPort, Scheme, Uri, Version};
use std::net::SocketAddr;
use uuid::Uuid;

#[derive(Clone)]
pub struct Config {
    pub server_addr: SocketAddr,
    pub own_addr: SocketAddr,

    pub username: String,
    pub password: String,
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