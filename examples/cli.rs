extern crate core;

use anyhow::Result;
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleRate, Stream, StreamConfig};
use log::LevelFilter;
use simplelog::Config as SimpleLogConfig;
use simplelog::{ColorChoice, CombinedLogger, TermLogger, TerminalMode};
use simple_sip_rs::call::{Call, CallControl, Media};
use simple_sip_rs::config::Config;
use simple_sip_rs::manager::SipManager;
use simple_sip_rs::call::outgoing_call::OutgoingCallResponse;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use futures_util::future::Either;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio::time::interval;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    pub server_address: String,
    #[clap(long)]
    pub own_address: String,

    #[clap(short, long)]
    pub username: String,
    #[clap(short, long)]
    pub password: String,
}

fn build_output_stream(buffer: Arc<Mutex<VecDeque<f32>>>) -> Stream {
    println!("{:?}", cpal::available_hosts());
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No output device available");
    let custom_config = StreamConfig {
        channels: 2,
        sample_rate: SampleRate(48000),
        buffer_size: BufferSize::Default,
    };

    device
        .build_output_stream(
            &custom_config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut samples = {
                    let mut audio_buffer = buffer.blocking_lock();
                    let end_index = if audio_buffer.len() > data.len() {
                        data.len()
                    } else {
                        audio_buffer.len()
                    };
                    audio_buffer.drain(..end_index).collect::<VecDeque<_>>()
                };

                for sample in data.iter_mut() {
                    if let Some(s) = samples.pop_front() {
                        *sample = cpal::Sample::from_sample(s);
                    } else {
                        *sample = cpal::Sample::from_sample(0.0);
                    }
                }
            },
            move |err| {
                println!("CPAL stream error {}", err);
            },
            None,
        )
        .unwrap()
}

fn build_input_stream(buffer: Arc<Mutex<VecDeque<f32>>>) -> Stream {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .expect("No input device available");
    let custom_config = StreamConfig {
        channels: 2,
        sample_rate: SampleRate(48000),
        buffer_size: BufferSize::Default,
    };

    device
        .build_input_stream(
            &custom_config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if buffer.blocking_lock().len() < 5_000 {
                    let mut samples = VecDeque::new();
                    samples.extend(data.iter().copied());
                    buffer.blocking_lock().append(&mut samples);
                }
            },
            move |err| {
                println!("CPAL stream error {}", err);
            },
            None,
        )
        .unwrap()
}

async fn handle_current_call(current_call: &mut Option<Call>, buffer_play: &Arc<Mutex<VecDeque<f32>>>) -> Result<()>
{
    match current_call.as_mut().unwrap().recv_either().await {
        Either::Left(message) => {
            if let Some(control) = message {
                println!("Received Control message {:?}", control);
                if control == CallControl::Finished {
                    drop(current_call.take());
                }
            }
        },
        Either::Right(media) => {
            if let Some(media) = media {
                match media {
                    Media::Audio(audio) => {
                        buffer_play.lock().await.append(&mut VecDeque::from(audio));


                    },
                    Media::TelephoneEvent(event) => {
                        println!("Received Telephone event {:?}, is key up {}", event.0, event.1);
                    },
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

async fn handle_command_input(line: String, sip_manager: &mut SipManager, current_call: &mut Option<Call>) -> Result<()>
{
    let command = line.split(" ").collect::<Vec<_>>();
    match command[0] {
        "accept" => {
            if let Ok(Some(c)) = sip_manager.recv_incoming_call().await {
                println!("Picked up call from {:?}", c.get_remote_uri());
                *current_call = Some(c.accept().await?);
            } else {
                println!("No pending calls");
            }
        },
        "reject" => {
            if let Ok(Some(c)) = sip_manager.recv_incoming_call().await {
                println!("Rejected call from {:?}", c.get_remote_uri());
                c.reject().await?;
            } else {
                println!("No pending calls");
            }
        }
        "hangup" => {
            if let Some(c) = current_call.as_ref() {
                c.hangup()?;
                println!("Hang up call");
            } else {
                println!("No call in progress");
            }
        }
        "call" => {
            if command.len() == 2 {
                let outgoing_call = sip_manager.call(command[1].to_string()).await?;
                println!("Calling {}", command[1]);
                match outgoing_call.wait_for_answer().await {
                    Ok(response) => {
                        match response {
                            OutgoingCallResponse::Accepted(call) => {
                                println!("Call has been accepted");
                                *current_call = Some(call);
                            },
                            OutgoingCallResponse::Rejected(status_code) => {
                                println!("Call has been rejected with status {}", status_code);
                            }
                        }
                    },
                    Err(e) => {
                        println!("Error while calling {}, {:?}", command[1], e);
                    }
                }
            } else {
                println!("Usage: 'call 1002'")
            }
        }
        &_ => {
            println!("accept, reject, hangup, call [NUMBER]");
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Debug,
        SimpleLogConfig::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )])
    .unwrap();

    let args = Args::parse();

    let config = Config {
        server_addr: SocketAddr::from_str(args.server_address.as_str()).unwrap(),
        own_addr: SocketAddr::from_str(args.own_address.as_str()).unwrap(),
        username: args.username.clone(),
        password: args.password.clone(),
    };

    let mut sip_manager =
        SipManager::from_config(config).await.unwrap();
    sip_manager.start().await.unwrap();

    let mut current_call: Option<Call> = None;

    let buffer_play: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let buffer_record: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));

    let output_stream = build_output_stream(buffer_play.clone());
    output_stream.play().expect("Failed to play output stream");
    let input_stream = build_input_stream(buffer_record.clone());
    input_stream.play().expect("Failed to play input stream");

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    let mut send_audio_interval = interval(Duration::from_millis(10));

    loop {
        tokio::select! {
            _ = send_audio_interval.tick() => {
                let samples = buffer_record.lock().await.drain(0..).collect::<Vec<_>>();

                if let Some(call) = current_call.as_mut() {
                    call.send_audio(samples).unwrap();
                }
            }
            res = async { handle_current_call(&mut current_call, &buffer_play).await }, if current_call.is_some() => {
                if let Err(err) = res {
                    println!("Error while handling call messages {}", err);
                }
            },
            line = lines.next_line() => {
                handle_command_input(line.unwrap().unwrap(), &mut sip_manager, &mut current_call).await.unwrap()
            }
        }
    }
}