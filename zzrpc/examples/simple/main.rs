use anyhow::Result;
use clap::Parser;

mod api;
mod client;
mod producer;
mod server;

#[derive(Debug, Parser)]
struct Args {
    /// Run as server (as opposed to client).
    #[arg(short, long)]
    server: bool,

    /// Server address.
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    address: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.server {
        server::run(&args.address).await?;
    } else {
        client::run(&args.address).await?;
    }

    Ok(())
}
