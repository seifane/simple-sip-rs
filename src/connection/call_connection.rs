use anyhow::Result;
use rsip::SipMessage;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{Receiver, Sender};

pub struct CallConnection {
    sender: Sender<SipMessage>,
    receiver: Receiver<SipMessage>,
}

impl CallConnection {
    pub fn new(sender: Sender<SipMessage>, receiver: Receiver<SipMessage>) -> CallConnection
    {
        CallConnection {
            sender,
            receiver,
        }
    }

    pub async fn send_message(&self, message: SipMessage) -> Result<()> {
        Ok(self.sender.send(message).await?)
    }

    pub async fn recv(&mut self) -> Option<SipMessage> {
        self.receiver.recv().await
    }

    pub fn try_recv(&mut self) -> Result<Option<SipMessage>> {
        match self.receiver.try_recv() {
            Ok(message) => {
                Ok(Some(message))
            }
            Err(err) => {
                match err {
                    TryRecvError::Empty => Ok(None),
                    TryRecvError::Disconnected => Err(err.into()),
                }
            }
        }
    }
}