# simple-sip-rs: A Tiny, Easy To Use, Experimental SIP Library for Rust

[![crates.io version](https://img.shields.io/crates/v/simple-sip-rs.svg)](https://crates.io/crates/simple-sip-rs)
[![docs.rs documentation](https://img.shields.io/docsrs/simple-sip-rs)](https://docs.rs/simple-sip-rs)

## Disclaimer
simple-sip-rs is currently in its early stages of development and is not suitable for production use. It is designed for small, experimental projects and may have limitations or bugs.

## Purpose
simple-sip-rs aims to provide a basic framework for implementing SIP (Session Initiation Protocol) functionality in Rust projects. It's a work in progress and may have limitations or bugs.

Note: simple-sip-rs is designed to be opinionated, making it easier for developers to get started with SIP in Rust.
However, this also means that it may not be as flexible as other libraries, and some concept may be abstracted.

## Features
- **Basic SIP message parsing and sending**: Can handle basic SIP messages like INVITE, ACK, BYE and CANCEL.
- **Support for PCMU, PCMA and Opus codecs**: simple-sip-rs can handle PCMU, PCMA and Opus codecs for audio communication. These can be enabled / disable through crate features.
- **Support for Telephone events**: simple-sip-rs can receive telephone events aka DTMF button presses (not send them currently).

## Usage

See the examples folder for a more complete example.

```rust
use std::net::SocketAddr;
use std::str::FromStr;
use simple_sip_rs::config::Config;
use simple_sip_rs::manager::SipManager;

async fn connect_and_call() {
    let config = Config {
        server_addr: SocketAddr::from_str("192.168.1.100:5060").unwrap(), // IP of SIP server
        own_addr: SocketAddr::from_str("192.168.1.2").unwrap(), // Your IP in relation to the SIP server
        username: "username".to_string(),
        password: "password".to_string(),
        rtp_port_start: 20400,
        rtp_port_end: 20500
    };
    
    
    let mut sip_manager = SipManager::from_config(config).await.unwrap();
    sip_manager.start().await.unwrap();
    
    let outgoing_call = sip_manager.call("1000".to_string());
}
```

## Crate features

- `opus`: Enables the Opus codec (default)
- `pcmu`: Enables the PCMU codec (default)s
- `pcma`: Enables the PCMA codec

## Examples

You can run the simple CLI example by running

```bash
cargo run --package simple-sip-rs --example cli -- --server-address 192.168.1.2:5060 --own-address 192.168.1.10:5060 --username username --password password
```

## Limitations and Future Plans

- Limited functionality: simple-sip-rs currently supports only a subset of SIP features. More features might be implemented over time. (PRs welcome)
- Experimental status: The API may change as the library evolves. Use with caution.
- Encryption: Encryption may be added in the future, but it's not guaranteed. (PRs welcome)
- Further testing: This was tested against a FreePBX server running in the same network as the client. There might be cases that are not
handled properly when operating behind NATs.

## Contributing
Contributions are welcome! Please feel free to open issues or submit pull requests.