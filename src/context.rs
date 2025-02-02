use crate::config::Config;

pub struct SipContext {
    pub config: Config,
    next_udp_port: u16,
}

impl SipContext {
    pub fn from_config(config: Config) -> Self
    {
        SipContext {
            config,
            next_udp_port: 20304,
        }
    }

    pub fn get_next_udp_port(&mut self) -> u16 {
        // TODO: check if the port is available first
        let port = self.next_udp_port;
        self.next_udp_port += 2;
        port
    }
}