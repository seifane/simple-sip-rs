use crate::call::incoming_call::IncomingCall;
use crate::call::outgoing_call::OutgoingCall;
use crate::config::Config;
use crate::connection::call_connection::CallConnection;
use crate::connection::sip_socket::SipSocket;
use crate::context::SipContext;

use crate::connection::socket_data::SocketData;
use anyhow::{anyhow, Result};
use rsip::Scheme::Sip;
use rsip::{HostWithPort, SipMessage, Uri};
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;


/// Receives incoming calls from the SIP server.
pub struct IncomingCallReceiver {
    receiver: Receiver<IncomingCall>,
}

impl IncomingCallReceiver {
    fn new(receiver: Receiver<IncomingCall>) -> Self {
        Self { receiver }
    }

    /// Receive the next incoming call.
    ///
    /// Returns `None` when the underlying connection was closed.
    pub async fn recv(&mut self) -> Option<IncomingCall>
    {
        self.receiver.recv().await
    }

    pub(crate) fn take(self) -> Receiver<IncomingCall> {
        self.receiver
    }
}

/// Represents an SIP session.
/// SipManager is used to instantiate the SIP connection make / receive calls.
///
/// Calling [start](SipManager::start) connects to the SIP Server and starts listening and handling SIP messages.
///
/// # Examples
/// ```
///  use std::net::SocketAddr;
///  use std::str::FromStr;
///  use simple_sip_rs::config::Config;
///  use simple_sip_rs::manager::SipManager;
///
///  async fn start_sip() {
///     let config = Config {
///         server_addr: SocketAddr::from_str("192.168.1.100:5060").unwrap(),
///         own_addr: SocketAddr::from_str("192.168.1.2").unwrap(),
///         username: "username".to_string(),
///         password: "password".to_string(),
///         rtp_port_start: 20480,
///         rtp_port_end: 20490,
///     };
///
///
///     let mut sip_manager = SipManager::from_config(config).await.unwrap();
///     sip_manager.start().await.unwrap();
///
///     let outgoing_call = sip_manager.call("1000".to_string());
/// }
/// ```
pub struct SipManager {
    context: Arc<Mutex<SipContext>>,

    incoming_call_receiver: Option<Receiver<IncomingCall>>,
    incoming_call_sender: Sender<IncomingCall>,

    inner: Option<InnerSipManager>
}

impl SipManager {
    /// Create SipManager from the config
    pub async fn from_config(config: Config) -> Result<Self> {
        let (sender, receiver) = tokio::sync::mpsc::channel(32);
        Ok(SipManager {
            context: Arc::new(Mutex::new(SipContext::from_config(config.clone())?)),

            incoming_call_receiver: Some(receiver),
            incoming_call_sender: sender,

            inner: None
        })
    }

    /// Starts the registration on the SIP server and starts listening to SIP messages.
    /// This function is non-blocking
    ///
    /// # Errors
    /// This function will return an error in the following cases:
    /// - Failed to establish the underlying TCP connection
    /// - Failed to authenticate
    pub async fn start(&mut self) -> Result<()> {
        self.stop();

        let inner = InnerSipManager::connect(
            self.context.clone(),
            self.incoming_call_sender.clone()
        ).await?;
        self.inner = Some(inner);

        Ok(())
    }

    /// Stops the underlying SIP socket. This effectively disconnects you from the server.
    pub fn stop(&mut self) {
        drop(self.inner.take());
    }

    /// Checks if the connection is alive.
    pub async fn is_running(&self) -> bool {
        if let Some(inner) = self.inner.as_ref() {
            return inner.is_running();
        }
        false
    }

    /// Takes the incoming call receiver.
    /// This is useful if you want to handle incoming calls in another task / thread.
    ///
    /// Will return None if the receiver was already taken.
    pub fn take_incoming_call_receiver(&mut self) -> Option<IncomingCallReceiver> {
        if let Some(receiver) = self.incoming_call_receiver.take() {
            return Some(IncomingCallReceiver::new(receiver));
        }
        None
    }

    /// Give back the incoming call receiver.
    pub fn give_incoming_call_receiver(&mut self, receiver: IncomingCallReceiver) {
        self.incoming_call_receiver = Some(receiver.take())
    }

    /// Get the next incoming call in the queue.
    ///
    /// # Errors
    ///
    /// Errors if the receiver was previously taken.
    pub async fn recv_incoming_call(&mut self) -> Result<Option<IncomingCall>>
    {
        if let Some(receiver) = self.incoming_call_receiver.as_mut() {
            return Ok(receiver.recv().await);
        }
        Err(anyhow!("Receiver was taken"))
    }

    /// Initiate a call to the given destination.
    ///
    /// # Arguments
    ///
    /// * `to`: Extension number to call. Ex: `"1000"`.
    ///
    /// # Errors
    ///
    /// This function will return an error in the following cases:
    /// - You are not connected to the server
    /// - Failure to send the Invite message
    pub async fn call(&self, to: String) -> Result<OutgoingCall>
    {
        if let Some(inner) = self.inner.as_ref() {
            return inner.call(to).await;
        }

        Err(anyhow!("Not connected"))
    }
}

struct InnerSipManager {
    context: Arc<Mutex<SipContext>>,

    socket_data: Arc<Mutex<SocketData>>,
    message_sender: Sender<SipMessage>,

    handle: JoinHandle<Result<()>>,
}

impl InnerSipManager {
    pub async fn connect(
        context: Arc<Mutex<SipContext>>,
        incoming_call_sender: Sender<IncomingCall>,
    ) -> Result<Self> {
        let addr = context.lock().await.config.server_addr.clone();
        let mut sip_socket = SipSocket::connect(addr, context.clone(), incoming_call_sender).await?;

        let socket_data = sip_socket.get_socket_data();
        let message_sender = sip_socket.get_message_sender();

        let handle = tokio::task::spawn(async move {
            sip_socket.run().await
        });

        Ok(Self {
            context,

            socket_data,
            message_sender,

            handle,
        })
    }

    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }

    pub fn stop(&mut self) {
        if !self.is_running() {
            self.handle.abort();
        }
    }

    pub async fn call(&self, to: String) -> Result<OutgoingCall> {
        let mut context_lock = self.context.lock().await;
        let to_uri = Uri {
            scheme: Some(Sip),
            auth: Some((to, Option::<String>::None).into()),
            host_with_port: HostWithPort::from(context_lock.config.server_addr),
            ..Default::default()
        };

        let call_id = Uuid::new_v4().to_string();
        let receiver = self.socket_data.lock().await.create_call_channel(call_id.clone()).await?;
        let call_connection = CallConnection::new(self.message_sender.clone(), receiver);

        OutgoingCall::try_from(context_lock.deref_mut(), call_connection, call_id, to_uri).await
    }
}

impl Drop for InnerSipManager {
    fn drop(&mut self) {
        self.stop();
    }
}