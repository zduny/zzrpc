use quote::quote;
use syn::ItemTrait;

use crate::{consumer_senders, consumer_state, modify_trait, request, response};

fn pretty(item: proc_macro2::TokenStream) -> String {
    let file = syn::parse_file(&item.to_string()).unwrap();
    prettyplease::unparse(&file)
}

fn compare(expected: proc_macro2::TokenStream, actual: proc_macro2::TokenStream) {
    let expected = pretty(expected);
    let actual = pretty(actual);

    if expected != actual {
        panic!("\n\nEXPECTED:\n\n{expected}\n\n\nFOUND:\n\n{actual}\n");
    }
}

fn api() -> proc_macro2::TokenStream {
    quote! {
        /// Public api.
        pub trait Api {
            /// Print "Hello World!" message on server.
            async fn hello_world(&self);

            /// Add two integers.
            async fn add_numbers(&self, a: i32, b: i32) -> i32;

            /// Concatenate two strings.
            async fn concatenate_strings(&self, a: String, b: String) -> String;

            /// Create stream of consequent natural numbers.
            async fn stream_natural_numbers(&self) -> impl Stream<Item = usize>;

            /// Keep sending server time at given interval.
            async fn stream_time(&self, interval: Duration) -> impl Stream<Item = NaiveTime>;
        }
    }
}

#[test]
fn test_request() {
    let expected = quote! {
        #[derive(Debug, zzrpc::serde::Serialize, zzrpc::serde::Deserialize)]
        pub enum Request {
            HelloWorld {},
            AddNumbers { a: i32, b: i32 },
            ConcatenateStrings { a: String, b: String },
            StreamNaturalNumbers {},
            StreamTime { interval: Duration },
        }
    };

    let item = syn::parse2::<ItemTrait>(api()).unwrap();
    let item = request(&item);
    let actual = quote! {
        #item
    };

    compare(expected, actual);
}

#[test]
fn test_response() {
    let expected = quote! {
        #[derive(Debug, zzrpc::serde::Serialize, zzrpc::serde::Deserialize)]
        pub enum Response {
            HelloWorld(()),
            AddNumbers(i32),
            ConcatenateStrings(String),
            StreamNaturalNumbers(Option<usize>),
            StreamTime(Option<NaiveTime>),
        }
    };

    let item = syn::parse2::<ItemTrait>(api()).unwrap();
    let item = response(&item);
    let actual = quote! {
        #item
    };

    compare(expected, actual);
}

#[test]
fn test_consumer_senders() {
    let expected = quote! {
        #[derive(Debug)]
        struct Senders<Error> {
            hello_world__sender: std::sync::Arc<
                zzrpc::futures::channel::mpsc::UnboundedSender<
                    (zzrpc::consumer::Message<Request>, zzrpc::consumer::ResultSender<(), Error>),
                >,
            >,
            add_numbers__sender: std::sync::Arc<
                zzrpc::futures::channel::mpsc::UnboundedSender<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<i32, Error>,
                    ),
                >,
            >,
            concatenate_strings__sender: std::sync::Arc<
                zzrpc::futures::channel::mpsc::UnboundedSender<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<String, Error>,
                    ),
                >,
            >,
            stream_natural_numbers__sender: std::sync::Arc<
                zzrpc::futures::channel::mpsc::UnboundedSender<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<usize, Error>,
                    ),
                >,
            >,
            stream_time__sender: std::sync::Arc<
                zzrpc::futures::channel::mpsc::UnboundedSender<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<NaiveTime, Error>,
                    ),
                >,
            >,
        }
    };

    let item = syn::parse2::<ItemTrait>(api()).unwrap();
    let item = consumer_senders(&item);
    let actual = quote! {
        #item
    };

    compare(expected, actual);
}

#[test]
fn test_consumer_state() {
    let expected = quote! {
        #[derive(Debug)]
        struct ConsumerState<Error> {
            hello_world__requests: std::collections::HashMap<
                usize,
                zzrpc::futures::channel::oneshot::Sender<zzrpc::consumer::Result<(), Error>>,
            >,
            hello_world__receiver: zzrpc::futures::channel::mpsc::UnboundedReceiver<
                (zzrpc::consumer::Message<Request>, zzrpc::consumer::ResultSender<(), Error>),
            >,
            add_numbers__requests: std::collections::HashMap<
                usize,
                zzrpc::futures::channel::oneshot::Sender<zzrpc::consumer::Result<i32, Error>>,
            >,
            add_numbers__receiver: zzrpc::futures::channel::mpsc::UnboundedReceiver<
                (zzrpc::consumer::Message<Request>, zzrpc::consumer::ResultSender<i32, Error>),
            >,
            concatenate_strings__requests: std::collections::HashMap<
                usize,
                zzrpc::futures::channel::oneshot::Sender<zzrpc::consumer::Result<String, Error>>,
            >,
            concatenate_strings__receiver: zzrpc::futures::channel::mpsc::UnboundedReceiver<
                (zzrpc::consumer::Message<Request>, zzrpc::consumer::ResultSender<String, Error>),
            >,
            stream_natural_numbers__requests: std::collections::HashMap<
                usize,
                zzrpc::futures::channel::mpsc::UnboundedSender<usize>,
            >,
            stream_natural_numbers__receiver: zzrpc::futures::channel::mpsc::UnboundedReceiver<
                (zzrpc::consumer::Message<Request>, zzrpc::consumer::ResultSender<usize, Error>),
            >,
            stream_time__requests: std::collections::HashMap<
                usize,
                zzrpc::futures::channel::mpsc::UnboundedSender<NaiveTime>,
            >,
            stream_time__receiver: zzrpc::futures::channel::mpsc::UnboundedReceiver<
                (
                    zzrpc::consumer::Message<Request>,
                    zzrpc::consumer::ResultSender<NaiveTime, Error>,
                ),
            >,
        }
        impl<Error> ConsumerState<Error> {
            fn new() -> (Senders<Error>, Self) {
                let (hello_world__sender, hello_world__receiver) = zzrpc::futures::channel::mpsc::unbounded::<
                    (zzrpc::consumer::Message<Request>, zzrpc::consumer::ResultSender<(), Error>),
                >();
                let (add_numbers__sender, add_numbers__receiver) = zzrpc::futures::channel::mpsc::unbounded::<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<i32, Error>,
                    ),
                >();
                let (concatenate_strings__sender, concatenate_strings__receiver) = zzrpc::futures::channel::mpsc::unbounded::<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<String, Error>,
                    ),
                >();
                let (stream_natural_numbers__sender, stream_natural_numbers__receiver) = zzrpc::futures::channel::mpsc::unbounded::<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<usize, Error>,
                    ),
                >();
                let (stream_time__sender, stream_time__receiver) = zzrpc::futures::channel::mpsc::unbounded::<
                    (
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<NaiveTime, Error>,
                    ),
                >();
                let senders = Senders {
                    hello_world__sender: std::sync::Arc::new(hello_world__sender),
                    add_numbers__sender: std::sync::Arc::new(add_numbers__sender),
                    concatenate_strings__sender: std::sync::Arc::new(
                        concatenate_strings__sender,
                    ),
                    stream_natural_numbers__sender: std::sync::Arc::new(
                        stream_natural_numbers__sender,
                    ),
                    stream_time__sender: std::sync::Arc::new(stream_time__sender),
                };
                let state = ConsumerState {
                    hello_world__requests: std::collections::HashMap::new(),
                    hello_world__receiver,
                    add_numbers__requests: std::collections::HashMap::new(),
                    add_numbers__receiver,
                    concatenate_strings__requests: std::collections::HashMap::new(),
                    concatenate_strings__receiver,
                    stream_natural_numbers__requests: std::collections::HashMap::new(),
                    stream_natural_numbers__receiver,
                    stream_time__requests: std::collections::HashMap::new(),
                    stream_time__receiver,
                };
                (senders, state)
            }
            fn handle_message(
                &mut self,
                message: zzrpc::producer::Message<Response>,
            ) -> Option<zzrpc::ShutdownType> {
                match message {
                    zzrpc::producer::Message::Response { id, response } => {
                        match response {
                            Response::HelloWorld(result) => {
                                if let Some(sender) = self.hello_world__requests.remove(&id) {
                                    let _ = sender.send(Ok(result));
                                }
                            }
                            Response::AddNumbers(result) => {
                                if let Some(sender) = self.add_numbers__requests.remove(&id) {
                                    let _ = sender.send(Ok(result));
                                }
                            }
                            Response::ConcatenateStrings(result) => {
                                if let Some(sender)
                                    = self.concatenate_strings__requests.remove(&id)
                                {
                                    let _ = sender.send(Ok(result));
                                }
                            }
                            Response::StreamNaturalNumbers(result) => {
                                if let Some(result) = result {
                                    if let Some(sender)
                                        = self.stream_natural_numbers__requests.get_mut(&id)
                                    {
                                        let _ = sender.unbounded_send(result);
                                    }
                                } else {
                                    self.stream_natural_numbers__requests.remove(&id);
                                }
                            }
                            Response::StreamTime(result) => {
                                if let Some(result) = result {
                                    if let Some(sender) = self.stream_time__requests.get_mut(&id)
                                    {
                                        let _ = sender.unbounded_send(result);
                                    }
                                } else {
                                    self.stream_time__requests.remove(&id);
                                }
                            }
                        }
                        None
                    }
                    zzrpc::producer::Message::Aborted => Some(zzrpc::ShutdownType::Aborted),
                    zzrpc::producer::Message::Shutdown => Some(zzrpc::ShutdownType::Shutdown),
                }
            }
            fn shutdown(&mut self, shutdown_type: zzrpc::ShutdownType) {
                for (_id, sender) in self.hello_world__requests.drain() {
                    let _ = sender.send(Err(shutdown_type.into()));
                }
                for (_id, sender) in self.add_numbers__requests.drain() {
                    let _ = sender.send(Err(shutdown_type.into()));
                }
                for (_id, sender) in self.concatenate_strings__requests.drain() {
                    let _ = sender.send(Err(shutdown_type.into()));
                }
            }
        }
    };

    let item = syn::parse2::<ItemTrait>(api()).unwrap();
    let item = consumer_state(&item);
    let actual = quote! {
        #item
    };

    compare(expected, actual);
}

#[test]
fn test_modify_trait() {
    let expected = quote! {
        /// Public api.
        pub trait Api {
            /// Print "Hello World!" message on server.
            #[must_use]
            fn hello_world(&self) -> zzrpc::ValueRequest<(), Request, Self::Error>;

            /// Add two integers.
            #[must_use]
            fn add_numbers(&self, a: i32, b: i32) -> zzrpc::ValueRequest<i32, Request, Self::Error>;

            /// Concatenate two strings.
            #[must_use]
            fn concatenate_strings(
                &self,
                a: String,
                b: String,
            ) -> zzrpc::ValueRequest<String, Request, Self::Error>;

            /// Create stream of consequent natural numbers.
            #[must_use]
            fn stream_natural_numbers(&self) -> zzrpc::StreamRequest<usize, Request, Self::Error>;

            /// Keep sending server time at given interval.
            #[must_use]
            fn stream_time(&self, interval: Duration) -> zzrpc::StreamRequest<NaiveTime, Request, Self::Error>;

            /// Request arguments.
            type Request;
            /// Transport error.
            type Error;
        }
    };

    let item = syn::parse2::<ItemTrait>(api()).unwrap();
    let item = modify_trait(item);
    let actual = quote! {
        #item
    };

    compare(expected, actual);
}
