use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use futures::{pin_mut, FutureExt};
use kodec::binary::Codec;
use mezzenger_tcp::Transport;
use tokio::{
    net::{TcpListener, TcpStream},
    select, spawn,
    sync::RwLock,
};
use tracing::{error, info, Level};
use zzrpc::producer::{Configuration, Produce};

use crate::producer::Producer;

#[derive(Debug, Default)]
struct State {}

pub async fn run(address: &str) -> Result<()> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("Server running!");
    let state = Arc::new(RwLock::new(State::default()));

    let listener = TcpListener::bind(&address).await?;
    info!("Listening at {}...", address);

    let break_signal = tokio::signal::ctrl_c().fuse();
    pin_mut!(break_signal);
    loop {
        select! {
            listener_result = listener.accept() => {
                let (stream, address) = listener_result?;
                let state = state.clone();
                spawn(async move {
                    tracing::debug!("accepted connection");
                    if let Err(error) = user_connected(stream, address, state).await {
                        error!("Error occurred: {error}");
                    }
                });
            },
            break_result = &mut break_signal => {
                break_result.expect("failed to listen for event");
                break;
            }
        }
    }
    info!("Shutting down...");

    Ok(())
}

async fn user_connected(
    stream: TcpStream,
    address: SocketAddr,
    _state: Arc<RwLock<State>>,
) -> Result<()> {
    let codec = Codec::default();
    let transport = Transport::new(stream, codec);
    let producer = Producer::new();
    info!("Producing for {address}...");
    let configuration = Configuration {
        timeout: Some(std::time::Duration::from_secs(3)),
        ..Default::default()
    };
    let shutdown = producer.produce(transport, configuration).await?;
    info!("Producing for {address} finished: {:?}.", shutdown);

    Ok(())
}
