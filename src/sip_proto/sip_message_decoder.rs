use bytes::{Buf, BytesMut};
use rsip::Header::ContentLength;
use rsip::prelude::HasHeaders;
use rsip::SipMessage;
use tokio_util::codec::Decoder;

const MAX_CONTENT_LENGTH: usize = 50 * 1000;

pub struct SipMessageDecoder {
    pending_message: Option<SipMessage>,
}

impl SipMessageDecoder {
    pub fn new() -> Self {
        Self { pending_message: None }
    }
}

impl Decoder for SipMessageDecoder {
    type Item = SipMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if self.pending_message.is_none() {
            if let Some(index) = src
                .windows(4)
                .enumerate()
                .find(|&(_, w)| matches!(w, b"\r\n\r\n"))
                .map(|(ix, _)| ix + 4) {
                if index == 4 {
                    // Received keep alive
                    src.advance(4);
                    return Ok(None)
                }

                let message = SipMessage::try_from(src.split_to(index).as_ref()).unwrap();
                self.pending_message = Some(message);
            }
        }

        if let Some(message) = self.pending_message.as_mut() {
            let content_length = (message
                .headers()
                .iter()
                .find_map(|header| {
                    if let ContentLength(header) = header {
                        Some(header.length().unwrap_or(0))
                    } else {
                        None
                    }
                })
                .unwrap_or(0) as usize)
                .min(MAX_CONTENT_LENGTH);

            if src.len() >= content_length {
                message.body_mut().append(&mut src.split_to(content_length).to_vec());
            } else {
                src.reserve(content_length - src.len());
            }

            if message.body().len() == content_length {
                return Ok(self.pending_message.take());
            }
        }

        Ok(None)
    }
}