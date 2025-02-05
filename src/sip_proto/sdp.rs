use crate::config::Config;
use crate::media::populate_sdp_media_from_codecs;
use anyhow::Result;
use webrtc_sdp::address::ExplicitlyTypedAddress;
use webrtc_sdp::attribute_type::SdpAttribute;
use webrtc_sdp::media_type::{SdpFormatList, SdpMedia, SdpMediaLine, SdpMediaValue, SdpProtocolValue};
use webrtc_sdp::{SdpConnection, SdpOrigin, SdpSession, SdpTiming};

pub fn generate_sdp_new(config: &Config, rtp_port: u16) -> Result<SdpSession>
{
    let mut session = SdpSession::new(0, SdpOrigin {
        username: "Z".to_string(),
        session_id: 0,
        session_version: 1234,
        unicast_addr: ExplicitlyTypedAddress::Ip(config.own_addr.ip()),
    }, "Z".to_string());

    session.set_connection(SdpConnection {
        address: ExplicitlyTypedAddress::Ip(config.own_addr.ip()),
        ttl: None,
        amount: None,
    });

    session.set_timing(SdpTiming {
        start: 0,
        stop: 0,
    });

    let mut media = SdpMedia::new(SdpMediaLine {
        media: SdpMediaValue::Audio,
        port: rtp_port as u32,
        port_count: 0,
        proto: SdpProtocolValue::RtpAvp,
        formats: SdpFormatList::Integers(vec![]),
    });
    populate_sdp_media_from_codecs(&mut media)?;

    media.add_attribute(SdpAttribute::Sendrecv)?;
    media.add_attribute(SdpAttribute::RtcpMux)?;
    session.extend_media(vec![media]);
    
    Ok(session)
}