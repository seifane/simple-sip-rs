use anyhow::Result;
use webrtc_sdp::address::ExplicitlyTypedAddress;
use webrtc_sdp::attribute_type::{SdpAttribute, SdpAttributeFmtp, SdpAttributeFmtpParameters, SdpAttributeRtpmap};
use webrtc_sdp::media_type::{SdpFormatList, SdpMedia, SdpMediaLine, SdpMediaValue, SdpProtocolValue};
use webrtc_sdp::{SdpConnection, SdpOrigin, SdpSession, SdpTiming};
use crate::config::Config;

fn add_opus(media: &mut SdpMedia) -> Result<()> {
    media.add_codec(SdpAttributeRtpmap {
        payload_type: 107,
        codec_name: "opus".to_string(),
        frequency: 48000,
        channels: Some(2),
    })?;

    media.add_attribute(SdpAttribute::Fmtp(SdpAttributeFmtp {
        payload_type: 107,
        parameters: SdpAttributeFmtpParameters {
            packetization_mode: 0,
            level_asymmetry_allowed: false,
            profile_level_id: 0,
            max_fs: 0,
            max_cpb: 0,
            max_dpb: 0,
            max_br: 0,
            max_mbps: 0,
            max_fr: 0,
            profile: None,
            level_idx: None,
            tier: None,
            maxplaybackrate: 48000,
            maxaveragebitrate: 0,
            usedtx: false,
            stereo: false,
            useinbandfec: true,
            cbr: false,
            ptime: 0,
            minptime: 0,
            maxptime: 0,
            encodings: vec![],
            dtmf_tones: "".to_string(),
            rtx: None,
            unknown_tokens: vec![],
        },
    }))?;

    Ok(())
}

fn add_telephone_event(media: &mut SdpMedia) -> Result<()> {
    media.add_codec(SdpAttributeRtpmap {
        payload_type: 101,
        codec_name: "telephone-event".to_string(),
        frequency: 8000,
        channels: None,
    })?;

    media.add_attribute(SdpAttribute::Fmtp(SdpAttributeFmtp {
        payload_type: 101,
        parameters: SdpAttributeFmtpParameters {
            packetization_mode: 0,
            level_asymmetry_allowed: false,
            profile_level_id: 0,
            max_fs: 0,
            max_cpb: 0,
            max_dpb: 0,
            max_br: 0,
            max_mbps: 0,
            max_fr: 0,
            profile: None,
            level_idx: None,
            tier: None,
            maxplaybackrate: 0,
            maxaveragebitrate: 0,
            usedtx: false,
            stereo: false,
            useinbandfec: true,
            cbr: false,
            ptime: 0,
            minptime: 0,
            maxptime: 0,
            encodings: vec![],
            dtmf_tones: "0-15".to_string(),
            rtx: None,
            unknown_tokens: vec![],
        },
    }))?;

    Ok(())
}

// TODO: Reinsert PCMU and add PCMA

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
    add_opus(&mut media)?;
    add_telephone_event(&mut media)?;

    media.add_attribute(SdpAttribute::Sendrecv)?;
    media.add_attribute(SdpAttribute::RtcpMux)?;
    session.extend_media(vec![media]);
    
    Ok(session)
}