use crate::media::RTPCodec;
use crate::call::Media;
use anyhow::Result;
use bytes::Bytes;
use fon::chan::Channel;
use fon::Audio;
use rtp::codecs::g7xx::G711Payloader;
use rtp::packet::Packet;
use rtp::packetizer::{new_packetizer, Packetizer};
use webrtc_sdp::attribute_type::SdpAttribute;
use webrtc_sdp::media_type::SdpMediaValue;
use webrtc_sdp::SdpSession;

fn encode(mut sample: i16) -> i8 {
    const ALAW_MAX: i16 = 0x0FFF;
    let mut mask: u16 = 0x0800;
    let mut sign: u8 = 0;
    let mut position: u8 = 11;
    if sample < 0 {
        sample = sample.overflowing_neg().0;
        sign = 0x80;
    }
    if sample > ALAW_MAX {
        sample = ALAW_MAX;
    }
    while (sample as u16 & mask) != mask && position >= 5 {
        mask >>= 1;
        position -= 1;
    }
    let lsb = if position == 4 {
        ((sample >> 1) & 0x0f) as u8
    } else {
        ((sample >> (position - 4)) & 0x0f) as u8
    };

    ((sign | ((position - 4) << 4) | lsb) ^ 0x55) as i8
}

fn decode(mut sample: i8) -> i16 {
    let mut sign: u8 = 0x00;

    sample ^= 0x55;
    if (sample as u8 & 0x80) > 0 {
        sample &= 0x7F;
        sign = 0x80;
    }

    let position = (((sample as u8 & 0xF0) as u8 >> 4) + 4) as u8;
    let decoded = if position != 4 {
        (1 << position) as i16
            | (((sample & 0x0F) as i16) << (position - 4))
            | (1 << (position - 5))
    } else {
        ((sample as i16) << 1) | 1
    };

    if sign == 0 {
        decoded
    } else {
        decoded.overflowing_neg().0
    }
}

pub struct PcmuCodec {
    ptime: u32,
    payload_type: u8,
    sample_rate: u32,
    channels: u8,

    packetizer: Box<dyn Packetizer + Send + Sync>,

    buffer_out: Vec<f32>,
}

impl PcmuCodec {
    pub fn try_from_sdp_session(sdp_session: &SdpSession) -> Result<Option<Self>> {
        for media in sdp_session.media.iter() {
            if media.get_type() != &SdpMediaValue::Audio {
                continue;
            }

            for attr in media.get_attributes().iter() {
                if let SdpAttribute::Rtpmap(a) = attr {
                    if a.codec_name.to_lowercase().as_str() == "pcmu" {
                        let instance = PcmuCodec {
                            ptime: 20,
                            payload_type: a.payload_type,
                            sample_rate: a.frequency,
                            channels: a.channels.unwrap_or(1) as u8,

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
            .map(|i| decode(i as i8))
            .collect::<Vec<_>>();
        let audio = Audio::<fon::chan::Ch16, 1>::with_i16_buffer(self.sample_rate, audio);

        let audio = Audio::<fon::chan::Ch32, 2>::with_audio(self.sample_rate, &audio)
            .iter()
            .map(|i| i.channels()[0].to_f32())
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
        let samples_count = (self.sample_rate / 1000 * self.ptime * self.channels as u32) as usize;
        let take_length = if self.buffer_out.len() < samples_count {
            self.buffer_out.len()
        } else {
            samples_count
        };

        let mut samples = self.buffer_out.drain(0..take_length).collect::<Vec<_>>();
        if samples.len() < samples_count {
            samples.extend(std::iter::repeat(0.0).take(take_length - samples.len()));
        }

        let audio = Audio::<fon::chan::Ch32, 2>::with_f32_buffer(self.sample_rate, samples);
        let audio = Audio::<fon::chan::Ch16, 1>::with_audio(self.sample_rate, &audio)
            .iter()
            .map(|i| {
                let i: i16 = i.channels()[0].into();
                encode(i) as u8
            })
            .collect::<Vec<_>>();
        let packets = self.packetizer.packetize(&Bytes::from(audio), self.sample_rate)?;
        Ok(packets)
    }
}