
//! sip-rs is a very simple library to interact with the SIP protocol and RTP.
//!
//! It's in very early stages, definitely not production ready.
//!
//! Right now it supports making, receiving calls from an SIP server over TCP transport.
//! UDP, any secure transports are not supported.
//!
//! Only audio calls are supported with either Opus or PCMU codec without encryption.
//!
//! To get started look at the [manager](manager::SipManager) module or example.

pub mod call;
pub mod config;
pub mod manager;

mod connection;
mod context;
mod sip_proto;
mod utils;
mod media;
