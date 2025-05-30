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
// https://github.com/kbalt/ezk-media/blob/main/crates/ezk-g711/src/mulaw.rs

fn encode(x: i16) -> u8 {
    let mut absno = if x < 0 {
        ((!x) >> 2) + 33
    } else {
        (x >> 2) + 33
    };

    if absno > 0x1FFF {
        absno = 0x1FFF;
    }

    let mut i = absno >> 6;
    let mut segno = 1;
    while i != 0 {
        segno += 1;
        i >>= 1;
    }

    let high_nibble = 0x8 - segno;
    let low_nibble = (absno >> segno) & 0xF;
    let low_nibble = 0xF - low_nibble;

    let mut ret = (high_nibble << 4) | low_nibble;

    if x >= 0 {
        ret |= 0x0080;
    }

    ret as u8
}

fn decode(y: u8) -> i16 {
    let y = y as i16;
    let sign: i16 = if y < 0x0080 { -1 } else { 1 };

    let mantissa = !y;
    let exponent = (mantissa >> 4) & 0x7;
    let segment = exponent + 1;
    let mantissa = mantissa & 0xF;

    let step = 4 << segment;

    sign * ((0x0080 << exponent) + step * mantissa + step / 2 - 4 * 33)
}
pub struct PcmuCodec {
    ptime: u32,
    payload_type: u8,
    sample_rate: u32,

    packetizer: Box<dyn Packetizer + Send + Sync>,

    buffer_out: Vec<f32>,
}

impl PcmuCodec {
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
                    if a.codec_name.to_lowercase().as_str() == "pcmu" {
                        let instance = PcmuCodec {
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

impl RTPCodec for PcmuCodec {
    fn populate_sdp_media(sdp_media: &mut SdpMedia) -> Result<()>
    where
        Self: Sized
    {
        sdp_media.add_codec(SdpAttributeRtpmap {
            payload_type: 0,
            codec_name: "PCMU".to_string(),
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