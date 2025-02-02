use anyhow::Result;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub struct BidirectionalChannel<T> {
    pub sender: UnboundedSender<T>,
    pub receiver: UnboundedReceiver<T>
}

impl <T: Send + Sync + 'static> BidirectionalChannel<T> {
    pub async fn recv(&mut self) -> Option<T> {
        self.receiver.recv().await
    }

    pub fn send(&mut self, value: T) -> Result<()> {
        Ok(self.sender.send(value)?)
    }

    pub fn one_sided(&self) -> bool {
        self.receiver.is_closed() || self.sender.is_closed()
    }
}

pub fn create_mpsc_bidirectional_unbounded<T>() -> (BidirectionalChannel<T>, BidirectionalChannel<T>)
{
    let (tx, rx) = unbounded_channel::<T>();
    let (tx1, rx1) = unbounded_channel::<T>();

    let first_channel = BidirectionalChannel {
        sender: tx,
        receiver: rx1,
    };
    let second_channel = BidirectionalChannel {
        sender: tx1,
        receiver: rx,
    };

    (first_channel, second_channel)
}