use crate::media::{RTPCodec};
use anyhow::Result;
use bytes::Bytes;
use opus::{Application, Channels, Decoder, Encoder};
use rtp::codecs::opus::OpusPayloader;
use rtp::packet::Packet;
use rtp::packetizer::{new_packetizer, Packetizer};
use webrtc_sdp::attribute_type::{SdpAttribute, SdpAttributeFmtp, SdpAttributeFmtpParameters, SdpAttributeRtpmap};
use webrtc_sdp::media_type::{SdpMedia, SdpMediaValue};
use webrtc_sdp::SdpSession;
use crate::call::Media;

pub struct OpusCodec {
    ptime: u32,

    payload_type: u8,
    sample_rate: u32,
    channels: u8,

    decoder: Decoder,
    encoder: Encoder,

    packetizer: Box<dyn Packetizer + Send + Sync>,

    buffer_out: Vec<f32>
}

impl OpusCodec {
    pub fn try_from_sdp_session(sdp_session: &SdpSession) -> Result<Option<Self>> {
        for media in sdp_session.media.iter() {
            if media.get_type() != &SdpMediaValue::Audio  {
                continue;
            }

            for attr in media.get_attributes().iter() {
                if let SdpAttribute::Rtpmap(a) = attr {
                    if a.codec_name.to_lowercase().as_str() == "opus" {
                        // TODO: Handle the fmtp params

                        let sample_rate = a.frequency;
                        let channels = a.channels.unwrap_or(1) as u8;
                        let channels_opus = match channels {
                            2 => Channels::Stereo,
                            _ => Channels::Mono
                        };
                        let instance = Self {
                            ptime: 20,
                            payload_type: a.payload_type,
                            sample_rate,
                            channels,
                            decoder: Decoder::new(sample_rate, channels_opus)?,
                            encoder:  Encoder::new(sample_rate, channels_opus, Application::Voip)?,

                            packetizer: Box::new(new_packetizer(
                                400,
                                a.payload_type,
                                rand::random::<u32>(),
                                Box::new(OpusPayloader::default()),
                                Box::new(rtp::sequence::new_random_sequencer()),
                                a.frequency
                            )),

                            buffer_out: vec![],
                        };

                        return Ok(Some(instance));
                    }
                }
            }
        }

        Ok(None)
    }
}

impl RTPCodec for OpusCodec {
    fn populate_sdp_media(sdp_media: &mut SdpMedia) -> Result<()>
    where
        Self: Sized
    {
        sdp_media.add_codec(SdpAttributeRtpmap {
            payload_type: 107,
            codec_name: "opus".to_string(),
            frequency: 48000,
            channels: Some(2),
        })?;

        sdp_media.add_attribute(SdpAttribute::Fmtp(SdpAttributeFmtp {
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

    fn get_payload_type(&self) -> u8 {
        self.payload_type
    }

    fn can_handle_media(&self, media: &Media) -> bool {
        if let Media::Audio(_) = media {
            return true
        }
        false
    }

    fn decode_payload(&mut self, payload: Bytes) -> Result<Option<Media>> {
        let payload = payload.to_vec();
        let nb_samples = self.decoder.get_nb_samples(payload.as_slice())? * self.channels as usize;
        let mut buffer = vec![0.0; nb_samples];
        self.decoder.decode_float(payload.as_slice(), buffer.as_mut_slice(), false)?;

        Ok(Some(Media::Audio(buffer)))
    }

    fn append_to_buffer(&mut self, media: Media) -> Result<()> {
        if let Media::Audio(mut buffer) = media {
            self.buffer_out.append(&mut buffer);
        }
        Ok(())
    }

    fn get_next_packet(&mut self) -> Result<Vec<Packet>> {
        if self.buffer_out.is_empty() {
            return Ok(vec![]);
        }
        let samples_count = (self.sample_rate / 1000 * self.ptime * self.channels as u32) as usize;

        let take_length = if self.buffer_out.len() < samples_count {
            self.buffer_out.len()
        } else {
            samples_count
        };

        let mut samples = self.buffer_out.drain(0..take_length).collect::<Vec<_>>();
        if samples.len() < samples_count  {
            samples.resize(samples_count, 0.0);
        }
        let payload = self.encoder.encode_vec_float(samples.as_slice(), samples.len())?;
        let packets = self.packetizer.packetize(&Bytes::from(payload), samples_count as u32)?;

        Ok(packets)
    }
}