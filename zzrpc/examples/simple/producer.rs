use std::time::Duration;

use chrono::{Local, NaiveTime};
use futures::Stream;
use zzrpc::Produce;

use crate::api::{impl_produce, Request, Response};

#[derive(Debug, Produce)]
pub struct Producer {}

impl Producer {
    pub fn new() -> Self {
        Producer {}
    }

    /// Print "Hello World!" message on the server.
    async fn hello_world(&self) {
        println!("Hello World!");
    }

    /// Add two numbers together.
    async fn add_numbers(&self, a: i32, b: i32) -> i32 {
        a + b
    }

    /// Concatenate two strings together.
    async fn concatenate_strings(&self, a: String, b: String) -> String {
        format!("{a}{b}")
    }

    /// Repeatedly send server time at given interval.
    async fn stream_time(&self, period: Duration) -> impl Stream<Item = NaiveTime> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            let mut tick = true;
            loop {
                interval.tick().await;
                println!("{}", if tick { "Tick! " } else { "Tock!" });
                tick = !tick;
                if sender.send(Local::now().time()).is_err() {
                    break;
                }
            }
        });
        tokio_stream::wrappers::UnboundedReceiverStream::new(receiver)
    }
}
