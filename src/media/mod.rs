pub mod opus;
pub mod pcmu;
pub mod telephone_events;

use anyhow::Result;
use bytes::Bytes;
use rtp::packet::Packet;
use webrtc_sdp::SdpSession;
use crate::call::Media;
use crate::media::opus::OpusCodec;
use crate::media::pcmu::PcmuCodec;
use crate::media::telephone_events::TelephoneEventsCodec;

pub trait RTPCodec {
    fn get_payload_type(&self) -> u8;
    fn can_handle_media(&self, media: &Media) -> bool;

    fn decode_payload(&mut self, payload: Bytes) -> Result<Option<Media>>;

    fn append_to_buffer(&mut self, media: Media) -> Result<()>;
    fn get_next_packet(&mut self) -> Result<Vec<Packet>>;
}

pub fn get_codecs_from_sdp_session(sdp_session: &SdpSession) -> Result<Vec<Box<dyn RTPCodec + Send>>>
{
    let mut codecs = Vec::new();

    if let Some(opus_codec) = OpusCodec::try_from_sdp_session(sdp_session)? {
        let boxed: Box<dyn RTPCodec + Send> = Box::new(opus_codec);
        codecs.push(boxed);
    }

    if let Some(pcmu_codec) = PcmuCodec::try_from_sdp_session(sdp_session)? {
        let boxed: Box<dyn RTPCodec + Send> = Box::new(pcmu_codec);
        codecs.push(boxed);
    }

    if let Some(telephone_events_codec) = TelephoneEventsCodec::try_from_sdp(sdp_session) {
        let boxed: Box<dyn RTPCodec + Send> = Box::new(telephone_events_codec);
        codecs.push(boxed);
    }

    Ok(codecs)
}