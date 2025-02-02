use anyhow::anyhow;
use rsip::SipMessage;
use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::mpsc;


// type WaitedIncomingMap = HashMap<String, oneshot::Sender<SipMessage>>;

#[derive(Default)]
pub struct SocketData {
    pub call_channels: HashMap<String, Sender<SipMessage>>,
}

impl SocketData {
    pub async fn create_call_channel(&mut self, call_id: String) -> anyhow::Result<Receiver<SipMessage>>
    {
        if self.call_channels.contains_key(&call_id) {
            return Err(anyhow!("A channel for this call id already exists: {}", call_id));
        }
        let (tx, rx) = mpsc::channel(32);
        self.call_channels.insert(call_id, tx);
        Ok(rx)
    }
}