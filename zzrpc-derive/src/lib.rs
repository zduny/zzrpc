use proc_macro::{self, TokenStream};
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

fn request() -> proc_macro2::TokenStream {
    let output = quote! {
        #[derive(Debug, Serialize, Deserialize)]
        pub enum Request {
            // ..
        }
    };
    output
}

fn response() -> proc_macro2::TokenStream {
    let output = quote! {
        #[derive(Debug, Serialize, Deserialize)]
        pub enum Response {
            // ..
        }
    };
    output
}

fn consumer_senders() -> proc_macro2::TokenStream {
    let output = quote! {
        #[derive(Debug)]
        struct Senders<Error> {
            // ..
        }
    };
    output
}

fn consumer_state() -> proc_macro2::TokenStream {
    let output = quote! {
        #[derive(Debug)]
        struct ConsumerState<Error> {
            // ..
        }
    };
    output
}

fn consumer() -> proc_macro2::TokenStream {
    let senders = consumer_senders();
    let state = consumer_state();

    let output = quote! {
        #senders

        #state

        #[derive(Debug)]
        pub struct Consumer<Error> {
            id_counter: ConsistentCounter,
            senders: Senders<Error>,
            drop_sender: Option<oneshot::Sender<()>>,
        }

        // ..

        impl<Error> Drop for Consumer<Error> {
            fn drop(&mut self) {
                if let Some(sender) = self.drop_sender.take() {
                    let _ = sender.send(());
                }
            }
        }
    };
    output
}

fn impl_produce() -> proc_macro2::TokenStream {
    let output = quote! {
        macro_rules! impl_produce {
            ($self:ident, $transport:ident, $configuration:ident) => {
            }
        }
    };
    output
}

#[proc_macro_attribute]
pub fn api(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item: proc_macro2::TokenStream = item.into();
    // todo: modify item
    
    let request = request();
    let response = response();
    let consumer = consumer();
    let impl_produce = impl_produce();

    let output = quote! {
        #item

        #request

        #response

        #consumer

        #impl_produce
    };
    output.into()
}

#[proc_macro_derive(Produce)]
pub fn produce(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, .. } = parse_macro_input!(input);
    let output = quote! {
        impl zzrpc::producer::Produce for #ident {
            type Request = Request;
            type Response = Response;
        
            #[cfg(not(target_arch = "wasm32"))]
            fn produce_unreliable<Transport, Error, Shutdown, SendErrorCallback, ReceiveErrorCallback>(
                self,
                transport: Transport,
                configuration: zzrpc::producer::Configuration<
                    Shutdown,
                    Error,
                    SendErrorCallback,
                    ReceiveErrorCallback,
                >,
            ) -> zzrpc::JoinHandle<ShutdownType> where
                Transport: mezzenger::Transport<
                        consumer::Message<Self::Request>,
                        zzrpc::Message<Self::Response>,
                        Error,
                    > + mezzenger::Reliable
                    + mezzenger::Order
                    + Send
                    + 'static,
                Shutdown: futures::Future<Output = ShutdownType> + Send + 'static,
                SendErrorCallback: zzrpc::SendErrorCallback<Error> + Send + 'static,
                ReceiveErrorCallback: zzrpc::ReceiveErrorCallback<Error> + Send + 'static {
                impl_produce!(self, transport, configuration)
            }
        
            #[cfg(target_arch = "wasm32")]
            fn produce_unreliable<Transport, Error, Shutdown, SendErrorCallback, ReceiveErrorCallback>(
                self,
                transport: Transport,
                configuration: zzrpc::producer::Configuration<
                    Shutdown,
                    Error,
                    SendErrorCallback,
                    ReceiveErrorCallback,
                >,
            ) -> zzrpc::JoinHandle<ShutdownType> where
                Transport: mezzenger::Transport<
                        consumer::Message<Self::Request>,
                        Message<Self::Response>,
                        Error,
                    > + mezzenger::Reliable
                    + mezzenger::Order
                    + 'static,
                Shutdown: futures::Future<Output = ShutdownType> + 'static,
                SendErrorCallback: zzrpc::SendErrorCallback<Error> + 'static,
                ReceiveErrorCallback: zzrpc::ReceiveErrorCallback<Error> + 'static {
                impl_produce!(self, transport, configuration)
            }
        }
    };
    output.into()
}
