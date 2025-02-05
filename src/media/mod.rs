#[cfg(feature = "opus")]
pub mod opus;
#[cfg(feature = "pcmu")]
pub mod pcmu;
#[cfg(feature = "pcma")]
pub mod pcma;
pub mod telephone_events;


use anyhow::Result;
use bytes::Bytes;
use rtp::packet::Packet;
use webrtc_sdp::media_type::SdpMedia;
use webrtc_sdp::SdpSession;
use crate::call::Media;
#[cfg(feature = "opus")]
use crate::media::opus::OpusCodec;
#[cfg(feature = "pcmu")]
use crate::media::pcmu::PcmuCodec;
#[cfg(feature = "pcma")]
use crate::media::pcma::PcmaCodec;
use crate::media::telephone_events::TelephoneEventsCodec;

pub trait RTPCodec {
    fn populate_sdp_media(sdp_media: &mut SdpMedia) -> Result<()> where Self: Sized;

    fn get_payload_type(&self) -> u8;
    fn can_handle_media(&self, media: &Media) -> bool;

    fn decode_payload(&mut self, payload: Bytes) -> Result<Option<Media>>;

    fn append_to_buffer(&mut self, media: Media) -> Result<()>;
    fn get_next_packet(&mut self) -> Result<Vec<Packet>>;
}

pub fn get_codecs_from_sdp_session(sdp_session: &SdpSession) -> Result<Vec<Box<dyn RTPCodec + Send>>>
{
    let mut codecs = Vec::new();

    #[cfg(feature = "opus")]
    if let Some(opus_codec) = OpusCodec::try_from_sdp_session(sdp_session)? {
        let boxed: Box<dyn RTPCodec + Send> = Box::new(opus_codec);
        codecs.push(boxed);
    }

    #[cfg(feature = "pcmu")]
    if let Some(pcmu_codec) = PcmuCodec::try_from_sdp_session(sdp_session)? {
        let boxed: Box<dyn RTPCodec + Send> = Box::new(pcmu_codec);
        codecs.push(boxed);
    }

    #[cfg(feature = "pcma")]
    if let Some(pcma_codec) = PcmaCodec::try_from_sdp_session(sdp_session)? {
        let boxed: Box<dyn RTPCodec + Send> = Box::new(pcma_codec);
        codecs.push(boxed);
    }

    if let Some(telephone_events_codec) = TelephoneEventsCodec::try_from_sdp(sdp_session) {
        let boxed: Box<dyn RTPCodec + Send> = Box::new(telephone_events_codec);
        codecs.push(boxed);
    }

    Ok(codecs)
}

pub fn populate_sdp_media_from_codecs(sdp_media: &mut SdpMedia) -> Result<()>
{
    #[cfg(feature = "opus")]
    OpusCodec::populate_sdp_media(sdp_media)?;
    #[cfg(feature = "pcmu")]
    PcmuCodec::populate_sdp_media(sdp_media)?;
    #[cfg(feature = "pcma")]
    PcmaCodec::populate_sdp_media(sdp_media)?;
    TelephoneEventsCodec::populate_sdp_media(sdp_media)?;

    Ok(())
}