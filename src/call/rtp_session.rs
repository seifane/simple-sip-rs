use anyhow::{anyhow, Result};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration};
use crate::media::{get_codecs_from_sdp_session, RTPCodec};
use log::{error, info};
use rtp::packet::Packet;
use tokio::net::UdpSocket;
use tokio::time::{interval, Interval};
use webrtc_sdp::address::ExplicitlyTypedAddress::Ip;
use webrtc_sdp::attribute_type::{SdpAttribute, SdpAttributeType};
use webrtc_util::{Conn, Marshal, Unmarshal};
use crate::call::session_parameters::SessionParameters;
use crate::call::Media;
use crate::utils::BidirectionalChannel;

pub struct RTPSession {
    audio_interval: Interval,

    udp_socket: UdpSocket,
    remote_addr: SocketAddr,

    codecs: Vec<Box<dyn RTPCodec + Send>>,

    media_channel: BidirectionalChannel<Media>,

    notified_empty: bool,
}

impl RTPSession {
    pub async fn new(
        media_channel: BidirectionalChannel<Media>,
        call_session_params: SessionParameters,
    ) -> Result<RTPSession> {
        let codecs = get_codecs_from_sdp_session(&call_session_params.remote.sdp)?;

        let udp_socket =
            UdpSocket::bind(
                SocketAddr::new(
                    IpAddr::V4(
                        Ipv4Addr::new(0, 0, 0, 0)
                    ),
                    call_session_params.local.port // TODO: Handle multiple media with multiple ports
                )
            ).await?;
        let media = call_session_params.remote.sdp.media.get(0).ok_or(anyhow!("no media found"))?;

        let remote_addr = if let Ip(ip) = call_session_params.remote.sdp.connection.as_ref().unwrap().address {
            Ok(SocketAddr::new(ip, media.get_port() as u16))
        } else {
            Err(anyhow!("Remote rtp ip address is not valid"))
        }?;

        let ptime = media.get_attribute(SdpAttributeType::Ptime).unwrap_or(&SdpAttribute::Ptime(20));
        let ptime = if let SdpAttribute::Ptime(ptime) = ptime {
            *ptime
        } else {
            20
        };

        Ok(RTPSession {
            audio_interval: interval(Duration::from_millis(ptime)),

            udp_socket,
            remote_addr,

            codecs,

            media_channel,
            notified_empty: true,
        })
    }

    pub async fn handle_next(&mut self) -> Result<()>
    {
        let mut buff = [0; 512];
        tokio::select! {
            _ = self.audio_interval.tick() => {
                self.send_next_packet().await?;
            },
            read_udp = self.udp_socket.recv_from(&mut buff) => {
                match read_udp {
                    Ok((len, _)) => {
                        let mut b = bytes::Bytes::from(buff[..len].to_vec());
                        let packet = Packet::unmarshal(&mut b)?;
                        if let Some(media) = self.receive_packet(packet).await? {
                            self.media_channel.sender.send(media)?;
                        }
                    }
                    Err(e) => {
                        error!("Error while receiving from rtp udp socket: {}", e);
                    }
                }
            }
            media_message = self.media_channel.receiver.recv() => {
                if let Some(media_message) = media_message {
                    self.receive_media(media_message).await?;
                }
            }
        }
        Ok(())
    }

    async fn receive_media(&mut self, media: Media) -> Result<()>
    {
        for codec in self.codecs.iter_mut() {
            if codec.can_handle_media(&media) {
                codec.append_to_buffer(media)?;
                return Ok(());
            }
        }
        Ok(())
    }

    async fn receive_packet(&mut self, packet: Packet) -> Result<Option<Media>>
    {
        for codec in self.codecs.iter_mut() {
            if codec.get_payload_type() == packet.header.payload_type {
                let media = codec.decode_payload(packet.payload.clone())?;
                return Ok(media);
            }
        }
        info!("Ignoring RTP Packet type {}", packet.header.payload_type);
        Ok(None)
    }

    async fn send_next_packet(&mut self) -> Result<()> {
        let mut did_send_packets = false;

        for codec in self.codecs.iter_mut() {
            let packets = codec.get_next_packet()?;
            if !packets.is_empty() {
                did_send_packets = true;
            }
            for packet in packets {
                let b = packet.marshal()?;
                self.udp_socket.send_to(b.iter().as_slice(), self.remote_addr).await?;
            }
        }

        if !did_send_packets {
            if !self.notified_empty {
                self.media_channel.sender.send(Media::OutputEmpty)?;
                self.notified_empty = true;
            }
        } else {
            self.notified_empty = false;
        }

        Ok(())
    }
}

impl Drop for RTPSession {
    fn drop(&mut self) {
        let _ = self.udp_socket.close();
    }
}

pub async fn rtp_task(
    media_channel: BidirectionalChannel<Media>,
    call_session_params: SessionParameters
) -> Result<()> {
    let mut session = RTPSession::new(media_channel, call_session_params).await?;

    loop {
        let res = session.handle_next().await;
        if let Err(err) = res {
            error!("rtp session error: {:?}", err);
        }
    }
}