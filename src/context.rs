use anyhow::{anyhow, Result};
use crate::config::Config;

pub struct SipContext {
    pub config: Config,
    next_udp_port: u16,
}

impl SipContext {
    pub fn from_config(config: Config) -> Result<Self>
    {
        if config.rtp_port_start > config.rtp_port_end {
            return Err(anyhow!("RTP start port is greater than RTP port end"));
        }

        Ok(SipContext {
            next_udp_port: config.rtp_port_start,
            config,
        })
    }

    pub fn get_next_udp_port(&mut self) -> u16 {
        // TODO: check if the port is available first
        let port = self.next_udp_port;
        self.next_udp_port += 2;
        if self.next_udp_port > self.config.rtp_port_end {
            self.next_udp_port = self.config.rtp_port_start;
        }
        port
    }
}