use std::collections::HashSet;
use anyhow::{anyhow, Result};
use bytes::Bytes;
use rtp::packet::Packet;
use webrtc_sdp::attribute_type::SdpAttribute;
use webrtc_sdp::media_type::SdpMediaValue;
use webrtc_sdp::SdpSession;
use crate::call::Media;
use crate::media::RTPCodec;

#[repr(u8)]
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TelephoneEvent {
    Zero = 0,
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Star = 10,
    Hash = 11,
    A = 12,
    B = 13,
    C = 14,
    D = 15,
}

impl TelephoneEvent {
    pub fn try_from_byte(b: &u8) -> Result<Self> {
        match b {
            0 => Ok(TelephoneEvent::Zero),
            1 => Ok(TelephoneEvent::One),
            2 => Ok(TelephoneEvent::Two),
            3 => Ok(TelephoneEvent::Three),
            4 => Ok(TelephoneEvent::Four),
            5 => Ok(TelephoneEvent::Five),
            6 => Ok(TelephoneEvent::Six),
            7 => Ok(TelephoneEvent::Seven),
            8 => Ok(TelephoneEvent::Eight),
            9 => Ok(TelephoneEvent::Nine),
            10 => Ok(TelephoneEvent::Star),
            11 => Ok(TelephoneEvent::Hash),
            12 => Ok(TelephoneEvent::A),
            13 => Ok(TelephoneEvent::B),
            14 => Ok(TelephoneEvent::C),
            15 => Ok(TelephoneEvent::D),
            _ => Err(anyhow::anyhow!("Invalid byte {}", b)),
        }
    }
}

pub struct TelephoneEventsCodec {
    payload_type: u8,
    pressed_keys: HashSet<TelephoneEvent>,
}

impl TelephoneEventsCodec {
    pub fn try_from_sdp(sdp_session: &SdpSession) -> Option<TelephoneEventsCodec> {
        for md in sdp_session.media.iter() {
            if md.get_type() != &SdpMediaValue::Audio {
                continue;
            }
            for attr in md.get_attributes() {
                if let SdpAttribute::Rtpmap(attr) = attr {
                    if attr.codec_name.to_lowercase().as_str() == "telephone-event" {
                        return Some(
                            TelephoneEventsCodec {
                                payload_type: attr.payload_type,
                                pressed_keys: HashSet::new()
                            }
                        )
                    }
                }
            }
        }
        None
    }
}

impl RTPCodec for TelephoneEventsCodec {
    fn get_payload_type(&self) -> u8 {
        self.payload_type
    }

    fn can_handle_media(&self, media: &Media) -> bool {
        if let Media::TelephoneEvent(_) = media {
            return true;
        }
        false
    }

    fn decode_payload(&mut self, payload: Bytes) -> Result<Option<Media>> {
        let event = TelephoneEvent::try_from_byte(
            payload.get(0).ok_or(anyhow!("Invalid main body"))?
        )?;
        let end = payload.get(1).ok_or(anyhow!("Invalid end"))? & 0b1000_0000 != 0;

        if !end && self.pressed_keys.contains(&event) {
            return Ok(None)
        }
        if end {
            self.pressed_keys.remove(&event);
        } else {
            self.pressed_keys.insert(event.clone());
        }

        Ok(Some(Media::TelephoneEvent((event, end))))
    }

    fn append_to_buffer(&mut self, _: Media) -> Result<()> {
        // TODO: Handle sending of telephone events
        Ok(())
    }

    fn get_next_packet(&mut self) -> Result<Vec<Packet>> {
        Ok(Vec::new())
    }
}

