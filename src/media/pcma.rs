use crate::media::RTPCodec;
use crate::call::Media;
use anyhow::Result;
use bytes::Bytes;
use fon::chan::Channel;
use fon::Audio;
use rtp::codecs::g7xx::G711Payloader;
use rtp::packet::Packet;
use rtp::packetizer::{new_packetizer, Packetizer};
use webrtc_sdp::attribute_type::{SdpAttribute, SdpAttributeRtpmap, SdpAttributeType};
use webrtc_sdp::media_type::{SdpMedia, SdpMediaValue};
use webrtc_sdp::SdpSession;

// Reference for encode / decode
// https://github.com/kbalt/ezk-media/blob/main/crates/ezk-g711/src/alaw.rs

fn encode(x: i16) -> u8 {
    let mut ix = if x < 0 { (!x) >> 4 } else { x >> 4 };

    if ix > 15 {
        let mut iexp = 1;

        while ix > 16 + 15 {
            ix >>= 1;
            iexp += 1;
        }
        ix -= 16;
        ix += iexp << 4;
    }

    if x >= 0 {
        ix |= 0x0080;
    }

    ((ix ^ 0x55) & 0xFF) as u8
}

fn decode(y: u8) -> i16 {
    let mut ix = y ^ 0x55;
    ix &= 0x7F;

    let iexp = ix >> 4;
    let mut mant = (ix & 0xF) as i16;
    if iexp > 0 {
        mant += 16;
    }

    mant = (mant << 4) + 0x8;

    if iexp > 1 {
        mant <<= iexp - 1;
    }

    if y > 127 {
        mant
    } else {
        -mant
    }
}
pub struct PcmaCodec {
    ptime: u32,
    payload_type: u8,
    sample_rate: u32,

    packetizer: Box<dyn Packetizer + Send + Sync>,

    buffer_out: Vec<f32>,
}

impl PcmaCodec {
    pub fn try_from_sdp_session(sdp_session: &SdpSession) -> Result<Option<Self>> {
        for media in sdp_session.media.iter() {
            if media.get_type() != &SdpMediaValue::Audio {
                continue;
            }

            let ptime = media.get_attribute(SdpAttributeType::Ptime).unwrap_or(&SdpAttribute::Ptime(20));
            let ptime = if let SdpAttribute::Ptime(ptime) = ptime {
                *ptime
            } else {
                20
            };

            for attr in media.get_attributes().iter() {
                if let SdpAttribute::Rtpmap(a) = attr {
                    if a.codec_name.to_lowercase().as_str() == "pcma" {
                        let instance = PcmaCodec {
                            ptime: ptime as u32,
                            payload_type: a.payload_type,
                            sample_rate: a.frequency,

                            packetizer: Box::new(new_packetizer(
                                300,
                                a.payload_type,
                                rand::random::<u32>(),
                                Box::new(G711Payloader::default()),
                                Box::new(rtp::sequence::new_random_sequencer()),
                                a.frequency,
                            )),
                            buffer_out: Vec::new(),
                        };

                        return Ok(Some(instance));
                    }
                }
            }
        }
        Ok(None)
    }
}

impl RTPCodec for PcmaCodec {
    fn populate_sdp_media(sdp_media: &mut SdpMedia) -> Result<()>
    where
        Self: Sized
    {
        sdp_media.add_codec(SdpAttributeRtpmap {
            payload_type: 0,
            codec_name: "PCMA".to_string(),
            frequency: 8000,
            channels: None,
        })?;

        Ok(())
    }

    fn get_payload_type(&self) -> u8 {
        self.payload_type
    }

    fn can_handle_media(&self, media: &Media) -> bool {
        if let Media::Audio(_) = media {
            return true;
        }
        false
    }

    fn decode_payload(&mut self, payload: Bytes) -> Result<Option<Media>> {
        let audio = payload
            .into_iter()
            .map(|i| decode(i))
            .collect::<Vec<_>>();
        let audio = Audio::<fon::chan::Ch16, 1>::with_i16_buffer(self.sample_rate, audio);

        let audio = Audio::<fon::chan::Ch32, 2>::with_audio(48000, &audio)
            .iter()
            .flat_map(|i| [i.channels()[0].to_f32(), i.channels()[1].to_f32()])
            .collect::<Vec<_>>();

        Ok(Some(Media::Audio(audio)))
    }

    fn append_to_buffer(&mut self, media: Media) -> Result<()> {
        if self.buffer_out.len() > 5000 {
            return Ok(());
        }
        if let Media::Audio(mut buffer) = media {
            self.buffer_out.append(&mut buffer);
        }
        Ok(())
    }

    fn get_next_packet(&mut self) -> Result<Vec<Packet>> {
        let samples_count = (48000 / 1000 * self.ptime * 2) as usize;
        let take_length = if self.buffer_out.len() < samples_count {
            self.buffer_out.len()
        } else {
            samples_count
        };

        let mut samples = self.buffer_out.drain(0..take_length).collect::<Vec<_>>();
        if samples.len() < samples_count {
            samples.extend(std::iter::repeat(0.0).take(take_length - samples.len()));
        }

        let audio = Audio::<fon::chan::Ch32, 2>::with_f32_buffer(48000, samples);
        let audio = Audio::<fon::chan::Ch16, 1>::with_audio(self.sample_rate, &audio)
            .iter()
            .map(|i| encode(i.channels()[0].into()))
            .collect::<Vec<_>>();
        let packets = self.packetizer.packetize(&Bytes::from(audio), self.sample_rate)?;
        Ok(packets)
    }
}