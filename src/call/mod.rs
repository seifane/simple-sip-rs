pub mod incoming_call;
pub mod outgoing_call;
mod call_handler;
mod session_parameters;
mod rtp_session;

use std::cmp::PartialEq;
use anyhow::{Context, Result};
use futures_util::future::Either;
use rsip::Uri;
use log::debug;
use tokio::task::JoinHandle;

use crate::call::session_parameters::SessionParameters;
use crate::call::call_handler::call_task;
use crate::call::rtp_session::rtp_task;
use crate::connection::call_connection::CallConnection;
use crate::media::telephone_events::TelephoneEvent;
use crate::utils::{create_mpsc_bidirectional_unbounded, BidirectionalChannel};

#[derive(Debug)]
pub enum Media {
    Audio(Vec<f32>),
    TelephoneEvent((TelephoneEvent, bool)),
    OutputEmpty,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CallControl {
    Hangup,
    AudioOutEmpty,
    Finished,
}

/// Represents an ongoing (as been answered) call.
pub struct Call {
    call_handle: JoinHandle<Result<()>>,
    rtp_handle: JoinHandle<Result<()>>,
    remote_uri: Uri,

    call_channel: BidirectionalChannel<CallControl>,
    media_channel: BidirectionalChannel<Media>,
}

impl Call {
    async fn new(call_connection: CallConnection, call_session_params: SessionParameters) -> Result<Self>
    {
        let (call_channel_local, call_channel_remote) = create_mpsc_bidirectional_unbounded();
        let (media_channel_local, media_channel_remote) = create_mpsc_bidirectional_unbounded();

        let remote_uri = call_session_params.remote.uri.clone();

        let cloned_call_session_params = call_session_params.clone();
        let call_handle = tokio::task::spawn(async move {
            let res = call_task(
                call_channel_remote,
                call_connection,
                cloned_call_session_params
            ).await;
            debug!("Call task finished with {:?}", res);
            res
        });

        let rtp_handle = tokio::task::spawn(async move {
            let res = rtp_task(media_channel_remote, call_session_params).await;
            debug!("RTP task finished with {:?}", res);
            res
        });

        Ok(Call {
            call_handle,
            rtp_handle,
            remote_uri,
            call_channel: call_channel_local,
            media_channel: media_channel_local,
        })
    }

    /// Blocks until the call has finished (hang up and terminated the worker thread)
    pub async fn block_for_finished(&mut self) {
        loop {
            match self.call_channel.recv().await {
                None => (),
                Some(control) => {
                    if control == CallControl::Finished {
                        return;
                    }
                }
            }
        }
    }

    /// Blocks until the output buffer is empty
    ///
    /// This is typically useful when sending already recorded sound,
    /// and you want to make sure the playback is finished before proceeding.
    pub async fn block_for_output_empty(&mut self) {
        loop {
            tokio::select! {
                call_message = self.call_channel.receiver.recv() => {
                    if let Some(control) = call_message {
                        if control == CallControl::Finished {
                            return;
                        }
                    }
                    return;
                }
                media = self.media_channel.receiver.recv() => {
                    if let Some(media) = media {
                        if let Media::OutputEmpty = media {
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Adds the given samples to the output audio buffer.
    ///
    /// # Arguments
    ///
    /// * `audio`: Interleaved stereo `f32` samples @ 48000Hz.
    ///
    /// # Errors
    /// Errors when failing to send the audio to the call. Most likely because the call has already ended.
    pub fn send_audio(&self, audio: Vec<f32>) -> Result<()>
    {
        self.media_channel.sender.send(Media::Audio(audio)).context("Failed to send audio to call. Call might be over.")
    }

    /// Tries to hang up the call. Might fail if the call is already over.
    pub fn hangup(&self) -> Result<()>
    {
        self.call_channel.sender.send(CallControl::Hangup).context("Failed to send hangup to call. Call might be over.")
    }

    /// Receive the next control message from the call. Blocking until a message arrives.
    pub async fn recv(&mut self) -> Option<CallControl>
    {
        self.call_channel.receiver.recv().await
    }

    /// Receive the next media message from the call. Blocking until a message arrives.
    pub async fn recv_media(&mut self) -> Option<Media> {
        self.media_channel.receiver.recv().await
    }

    /// Receive either the next control message or the next media message.
    pub async fn recv_either(&mut self) -> Either<Option<CallControl>, Option<Media>> {
        tokio::select! {
            message = self.call_channel.receiver.recv() => {
                Either::Left(message)
            }
            media = self.media_channel.receiver.recv() => {
                Either::Right(media)
            }
        }

    }

    /// Returns the remote URI
    pub fn get_remote_uri(&self) -> &String
    {
        &self.remote_uri.auth.as_ref().unwrap().user
    }

    /// Returns the state of the underlying worker
    ///
    /// `true` if the underlying worker as finished.
    pub fn is_finished(&self) -> bool {
        self.call_handle.is_finished() || self.rtp_handle.is_finished() || self.call_channel.one_sided() || self.media_channel.one_sided()
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        if !self.call_handle.is_finished() {
            self.call_handle.abort();
        }
        if !self.rtp_handle.is_finished() {
            self.rtp_handle.abort();
        }
    }
}