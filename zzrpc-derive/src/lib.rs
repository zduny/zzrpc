use convert_case::{Case, Casing};
use proc_macro::{self, TokenStream};
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{punctuated::Punctuated, token::Comma, Token};
use uuid::Uuid;

#[cfg(test)]
mod tests;

fn skip_self(
    arguments: &Punctuated<syn::FnArg, syn::token::Comma>,
) -> Punctuated<syn::FnArg, syn::token::Comma> {
    let mut output = Punctuated::new();
    for arg in arguments {
        if let syn::FnArg::Typed(pat_type) = &arg {
            if let syn::Pat::Ident(syn::PatIdent { ident, .. }) = &*pat_type.pat {
                if ident != "self" {
                    output.push(arg.clone());
                }
            }
        }
    }

    output
}

fn request(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let items = item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => {
            let ident = format_ident!("{}", method.sig.ident.to_string().to_case(Case::Pascal));
            let args = skip_self(&method.sig.inputs);
            let item = quote! {
                #ident { #args },
            };
            Some(item)
        }
        _ => None,
    });

    let output = quote! {
        #[derive(Debug, serde::Serialize, serde::Deserialize)]
        pub enum Request {
           #(#items)*
        }
    };
    output
}

fn extract_return_type(return_type: &syn::ReturnType) -> proc_macro2::TokenStream {
    match return_type {
        syn::ReturnType::Default => {
            quote! {
                ()
            }
        }
        syn::ReturnType::Type(_, return_type) => match *return_type.to_owned() {
            syn::Type::ImplTrait(impl_trait) => {
                let return_type = extract_stream_item_type(&impl_trait);
                quote! {
                    #return_type
                }
            }
            _ => {
                quote! {
                    #return_type
                }
            }
        },
    }
}

fn is_stream(return_type: &syn::ReturnType) -> bool {
    match return_type {
        syn::ReturnType::Default => false,
        syn::ReturnType::Type(_, return_type) => {
            matches!(*return_type.to_owned(), syn::Type::ImplTrait(_))
        }
    }
}

fn response(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let items = item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => {
            let ident = format_ident!("{}", method.sig.ident.to_string().to_case(Case::Pascal));
            let return_type = extract_return_type(&method.sig.output);
            let item = if is_stream(&method.sig.output) {
                quote! { #ident(zzrpc::producer::StreamResponse<#return_type>), }
            } else {
                quote! { #ident(#return_type), }
            };
            Some(item)
        }
        _ => None,
    });

    let output = quote! {
        #[derive(Debug, serde::Serialize, serde::Deserialize)]
        pub enum Response {
            #(#items)*
        }
    };
    output
}

fn consumer_senders(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let items = item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => {
            let ident = format_ident!(
                "{}__sender",
                method.sig.ident.to_string().to_case(Case::Snake)
            );
            let return_type = extract_return_type(&method.sig.output);
            let item = quote! {
                #ident: std::sync::Arc<
                    zzrpc::futures::channel::mpsc::UnboundedSender<(
                        zzrpc::consumer::Message<Request>,
                        zzrpc::consumer::ResultSender<#return_type, Error>,
                    )>,
                >,
            };
            Some(item)
        }
        _ => None,
    });

    let output = quote! {
        #[derive(Debug)]
        struct Senders<Error> {
            #(#items)*
        }
    };
    output
}

fn impl_consumer_state(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let mut channels = vec![];
    let mut senders = vec![];
    let mut items = vec![];
    let mut handlers = vec![];
    let mut drainers = vec![];

    for method in item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => Some(method),
        _ => None,
    }) {
        let ident = format_ident!("{}", method.sig.ident.to_string().to_case(Case::Pascal));
        let ident_requests = format_ident!(
            "{}__requests",
            method.sig.ident.to_string().to_case(Case::Snake)
        );
        let ident_sender = format_ident!(
            "{}__sender",
            method.sig.ident.to_string().to_case(Case::Snake)
        );
        let ident_receiver = format_ident!(
            "{}__receiver",
            method.sig.ident.to_string().to_case(Case::Snake)
        );
        let return_type = extract_return_type(&method.sig.output);

        channels.push(quote! {
            let (#ident_sender, #ident_receiver) = zzrpc::futures::channel::mpsc::unbounded::<(
                zzrpc::consumer::Message<Request>,
                zzrpc::consumer::ResultSender<#return_type, Error>,
            )>();
        });

        senders.push(quote! {
            #ident_sender: std::sync::Arc::new(#ident_sender),
        });

        items.push(quote! {
            #ident_requests: std::collections::HashMap::new(),
            #ident_receiver,
        });

        handlers.push(if is_stream(&method.sig.output) {
            quote! {
                Response::#ident(result) => {
                    match result {
                        zzrpc::producer::StreamResponse::Open => {
                            if let Some((sender, _)) = self.#ident_requests.get_mut(&id) {
                                if let Some(sender) = sender.take() {
                                    let _ = sender.send(Ok(()));
                                    self.pending -= 1;
                                }
                            }
                        },
                        zzrpc::producer::StreamResponse::Item(item) => {
                            if let Some((_, sender)) = self.#ident_requests.get_mut(&id) {
                                let _ = sender.unbounded_send(item);
                            }
                        },
                        zzrpc::producer::StreamResponse::Closed => {
                            self.#ident_requests.remove(&id);
                        },
                    }
                }
            }
        } else {
            quote! {
                Response::#ident(result) => {
                    if let Some(sender) = self.#ident_requests.remove(&id) {
                        let _ = sender.send(Ok(result));
                        self.pending -= 1;
                    }
                }
            }
        });

        if is_stream(&method.sig.output) {
            drainers.push(quote! {
                for (_id, (mut sender, _)) in self.#ident_requests.drain() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(Err(shutdown_type.into()));
                    }
                }
            });
        } else {
            drainers.push(quote! {
                for (_id, sender) in self.#ident_requests.drain() {
                    let _ = sender.send(Err(shutdown_type.into()));
                }
            });
        }
    }

    let output = quote! {
        impl<Error> ConsumerState<Error> {
            fn new() -> (Senders<Error>, Self) {
                #(#channels)*

                let senders = Senders {
                    #(#senders)*
                };

                let state = ConsumerState {
                    pending: 0,
                    #(#items)*
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
                            #(#handlers)*
                        }
                        None
                    }
                    zzrpc::producer::Message::Aborted => Some(zzrpc::ShutdownType::Aborted),
                    zzrpc::producer::Message::Shutdown => Some(zzrpc::ShutdownType::Shutdown),
                }
            }

            fn idle(&self) -> bool {
                self.pending == 0
            }

            fn shutdown(&mut self, shutdown_type: zzrpc::ShutdownType) {
                #(#drainers)*
            }
        }
    };
    output
}

fn consumer_state(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let items = item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => {
            let ident_requests = format_ident!("{}__requests", method.sig.ident.to_string().to_case(Case::Snake));
            let ident_receiver = format_ident!("{}__receiver", method.sig.ident.to_string().to_case(Case::Snake));
            let return_type = extract_return_type(&method.sig.output);

            let sender = if is_stream(&method.sig.output) {
                quote! {
                    (Option<zzrpc::futures::channel::oneshot::Sender<zzrpc::consumer::Result<(), Error>>>,
                    zzrpc::futures::channel::mpsc::UnboundedSender<#return_type>)
                }
            } else {
                quote! {
                    zzrpc::futures::channel::oneshot::Sender<zzrpc::consumer::Result<#return_type, Error>>
                }
            };

            let item = quote! {
                #ident_requests: std::collections::HashMap<usize, #sender>,
                #ident_receiver: zzrpc::futures::channel::mpsc::UnboundedReceiver<(
                    zzrpc::consumer::Message<Request>,
                    zzrpc::consumer::ResultSender<#return_type, Error>,
                )>,
            };
            Some(item)
        }
        _ => None,
    });

    let impl_consumer_state = impl_consumer_state(item);

    let output = quote! {
        #[derive(Debug)]
        struct ConsumerState<Error> {
            pending: usize,
            #(#items)*
        }

        #impl_consumer_state
    };
    output
}

fn impl_consume(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let items = item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => {
            let ident_option = format_ident!(
                "{}__option",
                method.sig.ident.to_string().to_case(Case::Snake)
            );
            let ident_receiver = format_ident!(
                "{}__receiver",
                method.sig.ident.to_string().to_case(Case::Snake)
            );
            let ident_requests = format_ident!(
                "{}__requests",
                method.sig.ident.to_string().to_case(Case::Snake)
            );

            let patterns = if is_stream(&method.sig.output) {
                quote! {
                    zzrpc::consumer::ResultSender::Stream { result_sender, values_sender } => {
                        if let Err(error) = result {
                            let _ = result_sender.send(Err(error));
                        } else {
                            state.#ident_requests.insert(id, (Some(result_sender), values_sender));
                            state.pending += 1;
                        }
                    },
                    zzrpc::consumer::ResultSender::Abort => {
                        if let Some((sender, _)) = state.#ident_requests.remove(&id) {
                            if sender.is_some() {
                                state.pending -= 1;
                            }
                        }
                    },
                    _ => unreachable!("value sender got when stream sender expected"),
                }
            } else {
                quote! {
                    zzrpc::consumer::ResultSender::Value(sender) => {
                        if let Err(error) = result {
                            let _ = sender.send(Err(error));
                        } else {
                            state.#ident_requests.insert(id, sender);
                            state.pending += 1;
                        }
                    },
                    zzrpc::consumer::ResultSender::Abort => {
                        if state.#ident_requests.remove(&id).is_some() {
                            state.pending -= 1;
                        }
                    },
                    _ => unreachable!("stream sender got when value sender expected"),
                }
            };

            let item = quote! {
                #ident_option = zzrpc::futures::StreamExt::next(&mut state.#ident_receiver) => {
                    if let Some((message, result_sender)) = #ident_option {
                        if timeout.is_some() {
                            timeout_future.reset();
                        }

                        let id = message.id;
                        let result = sender.send(message).await;
                        let result = result.map_err(|error| {
                            match error {
                                mezzenger::Error::Closed => zzrpc::Error::Closed,
                                mezzenger::Error::Other(error) => zzrpc::Error::Transport(error),
                            }
                        });
                        match result_sender {
                            #patterns
                        }
                    }
                },
            };
            Some(item)
        }
        _ => None,
    });

    let implementation = quote! {
        use zzrpc::futures::{FutureExt, SinkExt, StreamExt};

        let zzrpc::consumer::Configuration {
            shutdown,
            mut receive_error_callback,
            timeout,
            ..
        } = configuration;

        let (drop_sender, mut drop_receiver) = zzrpc::futures::channel::oneshot::channel::<()>();
        let drop_sender = Some(drop_sender);

        let (senders, mut state) = ConsumerState::new();

        zzrpc::spawn(async move {
            let (mut sender, receiver) = transport.split();
            let receiver = zzrpc::futures::StreamExt::fuse(receiver);

            let timeout_future = if let Some(duration) = timeout {
                zzrpc::Timeout::new(duration)
            } else {
                zzrpc::Timeout::never()
            };
            let shutdown = zzrpc::futures::FutureExt::fuse(shutdown);
            zzrpc::futures::pin_mut!(receiver, timeout_future, shutdown);

            loop {
                zzrpc::futures::select! {
                    receive_option = zzrpc::futures::StreamExt::next(&mut receiver) => {
                        if let Some(receive_result) = receive_option {
                            if timeout.is_some() {
                                timeout_future.reset();
                            }

                            match receive_result {
                                Ok(message) =>  {
                                    if let Some(shutdown_type) = state.handle_message(message) {
                                        state.shutdown(shutdown_type);
                                        break;
                                    }
                                },
                                Err(error) => {
                                    if let zzrpc::HandlingStrategy::Stop(shutdown_type) = receive_error_callback.on_receive_error(error) {
                                        state.shutdown(shutdown_type);
                                        break;
                                    }
                                },
                            }
                        } else {
                            state.shutdown(zzrpc::ShutdownType::Closed);
                            break;
                        }
                    },
                    #(#items)*
                    _ = &mut timeout_future => {
                        if timeout.is_some() && !state.idle() {
                            state.shutdown(zzrpc::ShutdownType::Timeout);
                            break;
                        }
                    },
                    shutdown_type = &mut shutdown => {
                        state.shutdown(shutdown_type);
                        break;
                    }
                    _ = &mut drop_receiver => { break; }
                }
            }
        });

        Consumer {
            id_counter: zzrpc::atomic_counter::ConsistentCounter::new(0),
            senders,
            drop_sender,
        }
    };

    let output = quote! {
        impl<Error> zzrpc::consumer::Consume<Consumer<Error>, Error> for Consumer<Error> {
            type Request = Request;
            type Response = Response;

            #[cfg(not(target_arch = "wasm32"))]
            fn consume_unreliable<Transport, Shutdown, ReceiveErrorCallback>(
                transport: Transport,
                configuration: zzrpc::consumer::Configuration<Shutdown, Error, ReceiveErrorCallback>,
            ) -> Consumer<Error>
            where
                Transport: mezzenger::Transport<
                        zzrpc::producer::Message<Self::Response>,
                        zzrpc::consumer::Message<Self::Request>,
                        Error,
                    > + mezzenger::Reliable
                    + mezzenger::Order
                    + Send
                    + 'static,
                Shutdown: zzrpc::futures::Future<Output = zzrpc::ShutdownType> + Send + 'static,
                ReceiveErrorCallback: zzrpc::ReceiveErrorCallback<Error> + Send + 'static,
                Error: Send + 'static, {
                #implementation
            }

            #[cfg(target_arch = "wasm32")]
            fn consume_unreliable<Transport, Shutdown, ReceiveErrorCallback>(
                transport: Transport,
                configuration: zzrpc::consumer::Configuration<Shutdown, Error, ReceiveErrorCallback>,
            ) -> Consumer<Error>
            where
                Transport: mezzenger::Transport<
                        zzrpc::producer::Message<Self::Response>,
                        zzrpc::consumer::Message<Self::Request>,
                        Error,
                    > + mezzenger::Reliable
                    + mezzenger::Order
                    + 'static,
                Shutdown: zzrpc::futures::Future<Output = zzrpc::ShutdownType> + 'static,
                ReceiveErrorCallback: zzrpc::ReceiveErrorCallback<Error> + 'static,
                Error: 'static, {
                #implementation
            }
        }
    };
    output
}

fn pattern_arguments(
    arguments: &Punctuated<syn::FnArg, syn::token::Comma>,
) -> Punctuated<syn::Ident, Comma> {
    let mut output = Punctuated::new();
    for arg in arguments {
        if let syn::FnArg::Typed(pat_type) = &arg {
            if let syn::Pat::Ident(syn::PatIdent { ident, .. }) = &*pat_type.pat {
                if ident != "self" {
                    output.push(ident.clone());
                }
            }
        }
    }

    output
}

fn impl_api(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let ident = &item.ident;

    let items = item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => {
            let ident_request =
                format_ident!("{}", method.sig.ident.to_string().to_case(Case::Pascal));
            let ident_sender = format_ident!(
                "{}__sender",
                method.sig.ident.to_string().to_case(Case::Snake)
            );

            let mut signature = method.sig.clone();
            signature.asyncness = None;
            match &signature.output {
                syn::ReturnType::Default => {
                    signature.output = syn::parse2::<syn::ReturnType>(
                        quote!(-> zzrpc::ValueRequest<(), Request, Self::Error>),
                    )
                    .unwrap();
                }
                syn::ReturnType::Type(_, return_type) => match *return_type.to_owned() {
                    syn::Type::ImplTrait(impl_trait) => {
                        let return_type = extract_stream_item_type(&impl_trait);
                        signature.output = syn::parse2::<syn::ReturnType>(
                            quote!(-> zzrpc::StreamRequest<#return_type, Request, Self::Error>),
                        )
                        .unwrap();
                    }
                    _ => {
                        signature.output = syn::parse2::<syn::ReturnType>(
                            quote!(-> zzrpc::ValueRequest<#return_type, Request, Self::Error>),
                        )
                        .unwrap();
                    }
                },
            };

            let ident_request_future = if is_stream(&method.sig.output) {
                quote!(zzrpc::StreamRequest)
            } else {
                quote!(zzrpc::ValueRequest)
            };

            let arguments = pattern_arguments(&method.sig.inputs);

            let item = quote! {
                #signature {
                    use zzrpc::atomic_counter::AtomicCounter;
                    let request = Request::#ident_request { #arguments };
                    #ident_request_future::new(
                        self.senders.#ident_sender.clone(),
                        self.id_counter.inc(),
                        request,
                    )
                }
            };
            Some(item)
        }
        _ => None,
    });

    let output = quote! {
        impl<Error> #ident for Consumer<Error> {
            #(#items)*

            type Request = Request;
            type Error = Error;
        }
    };
    output
}

fn consumer(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let senders = consumer_senders(item);
    let state = consumer_state(item);

    let impl_consume = impl_consume(item);
    let impl_api = impl_api(item);

    let output = quote! {
        #senders

        #state

        #[derive(Debug)]
        pub struct Consumer<Error> {
            id_counter: zzrpc::atomic_counter::ConsistentCounter,
            senders: Senders<Error>,
            drop_sender: Option<zzrpc::futures::channel::oneshot::Sender<()>>,
        }

        #impl_consume

        #impl_api

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

fn impl_produce(item: &syn::ItemTrait) -> proc_macro2::TokenStream {
    let items = item.items.iter().filter_map(|item| match item {
        syn::TraitItem::Method(method) => {
            let ident = method.sig.ident.clone();
            let ident_request =
                format_ident!("{}", method.sig.ident.to_string().to_case(Case::Pascal));

            let arguments = pattern_arguments(&method.sig.inputs);

            let item = if is_stream(&method.sig.output) {
                quote! {
                    Request::#ident_request { #arguments } => {
                        let me = me.clone();
                        let reply_sender = reply_sender.clone();
                        let remove_aborter_sender = remove_aborter_sender.clone();
                        spawn(async move {
                            let mut stream = zzrpc::futures::StreamExt::fuse(me.#ident(#arguments).await);

                            let response = zzrpc::producer::StreamResponse::Open;
                            let response = Response::#ident_request(response);
                            let message = zzrpc::producer::Message::Response { id, response };
                            let _ = reply_sender.unbounded_send(message);
                            
                            loop {
                                select! {
                                    result = zzrpc::futures::StreamExt::next(&mut stream) => {
                                        if let Some(result) = result {
                                            let response = zzrpc::producer::StreamResponse::Item(result);
                                            let response = Response::#ident_request(response);
                                            let message = zzrpc::producer::Message::Response { id, response };
                                            let _ = reply_sender.unbounded_send(message);
                                        } else {
                                            let response = zzrpc::producer::StreamResponse::Closed;
                                            let response = Response::#ident_request(response);
                                            let message = zzrpc::producer::Message::Response { id, response };
                                            let _ = reply_sender.unbounded_send(message);

                                            let _ = remove_aborter_sender.unbounded_send(id);
                                            break;
                                        }
                                    },
                                    _ = abort_receiver => {
                                        break;
                                    },
                                };
                            }
                        });
                    },
                }
            } else {
                quote! {
                    Request::#ident_request { #arguments } => {
                        let me = me.clone();
                        let reply_sender = reply_sender.clone();
                        let remove_aborter_sender = remove_aborter_sender.clone();
                        spawn(async move {
                            let task = zzrpc::futures::FutureExt::fuse(me.#ident(#arguments));
                            pin_mut!(task);
                            select! {
                                result = task => {
                                    let response = Response::#ident_request(result);
                                    let message = zzrpc::producer::Message::Response { id, response };
                                    let _ = reply_sender.unbounded_send(message);
                                    let _ = remove_aborter_sender.unbounded_send(id);
                                },
                                _ = abort_receiver => (),
                            };
                        });
                    },
                }
            };

            Some(item)
        }
        _ => None,
    });

    let uuid = Uuid::new_v4();
    let ident = format_ident!("__impl_produce_{}", uuid.simple().to_string());

    let output = quote! {
        #[macro_export]
        macro_rules! #ident {
            ($self:ident, $transport:ident, $configuration:ident) => {
                zzrpc::spawn(async move {
                    use std::sync::Arc;
                    use std::collections::HashMap;
                    use futures::{
                        channel::{
                            mpsc::unbounded,
                            oneshot,
                        },
                        pin_mut, select, SinkExt, StreamExt,
                    };
                    use zzrpc::{ShutdownType, Timeout};

                    use zzrpc::spawn;

                    let me = Arc::new($self);

                    let (mut sender, receiver) = $transport.split();
                    let mut receiver = zzrpc::futures::StreamExt::fuse(receiver);

                    let (reply_sender, mut reply_receiver) = unbounded::<zzrpc::producer::Message<Response>>();

                    let mut aborters: HashMap<usize, oneshot::Sender<()>> = HashMap::new();
                    let (remove_aborter_sender, mut remove_aborter_receiver) = unbounded::<usize>();

                    let (stop_sender, stop_receiver) = oneshot::channel::<ShutdownType>();
                    let mut stop_sender = Some(stop_sender);

                    let zzrpc::producer::Configuration {
                        shutdown,
                        mut send_error_callback,
                        mut receive_error_callback,
                        timeout,
                        ..
                    } = $configuration;

                    spawn(async move {
                        while let Some(message) = zzrpc::futures::StreamExt::next(&mut reply_receiver).await {
                            match message {
                                zzrpc::producer::Message::Response { id, .. } => {
                                    let result = sender.send(message).await;
                                    if let Err(error) = result {
                                        if let zzrpc::HandlingStrategy::Stop(shutdown_type) =
                                            send_error_callback.on_send_error(id, error)
                                        {
                                            if let Some(stop_sender) = stop_sender.take() {
                                                let _ = stop_sender.send(shutdown_type);
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    let _ = sender.send(message).await;
                                }
                            }
                        }
                    });

                    let handle_message = |aborters: &mut HashMap<usize, oneshot::Sender<()>>, message: zzrpc::consumer::Message<Request>| {
                        let zzrpc::consumer::Message { id, payload } = message;
                        match payload {
                            zzrpc::consumer::Payload::Request(request) => {
                                let (abort_sender, mut abort_receiver) = oneshot::channel::<()>();
                                aborters.insert(id, abort_sender);
                                match request {
                                     #(#items)*
                                }
                            }
                            zzrpc::consumer::Payload::Abort => {
                                if let Some(abort_sender) = aborters.remove(&id) {
                                    let _ = abort_sender.send(());
                                }
                            }
                        }
                    };

                    let mut return_value = ShutdownType::Closed;
                    let mut handle_shutdown = |shutdown_type: ShutdownType| {
                        return_value = shutdown_type;
                        let message = match shutdown_type {
                            ShutdownType::Shutdown => zzrpc::producer::Message::Shutdown,
                            ShutdownType::Aborted => zzrpc::producer::Message::Aborted,
                            _ => {
                                return;
                            }
                        };
                        let _ = reply_sender.unbounded_send(message);
                    };

                    let timeout_future = if let Some(duration) = timeout {
                        Timeout::new(duration)
                    } else {
                        Timeout::never()
                    };
                    let shutdown = zzrpc::futures::FutureExt::fuse(shutdown);
                    pin_mut!(shutdown, timeout_future, stop_receiver);
                    loop {
                        select! {
                            receive_option = zzrpc::futures::StreamExt::next(&mut receiver) => {
                                if timeout.is_some() {
                                    timeout_future.reset();
                                }

                                if let Some(receive_result) = receive_option {
                                    match receive_result {
                                        Ok(message) => handle_message(&mut aborters, message),
                                        Err(error) => {
                                            if let zzrpc::HandlingStrategy::Stop(shutdown_type) = receive_error_callback.on_receive_error(error) {
                                                handle_shutdown(shutdown_type);
                                                break;
                                            }
                                        },
                                    }
                                } else {
                                    handle_shutdown(ShutdownType::Closed);
                                    break;
                                }
                            },
                            id_option = zzrpc::futures::StreamExt::next(&mut remove_aborter_receiver) => {
                                if let Some(id) = id_option {
                                    aborters.remove(&id);
                                }
                            },
                            _ = &mut timeout_future => {
                                if timeout.is_some() {
                                    if aborters.is_empty() {
                                        handle_shutdown(ShutdownType::Timeout);
                                        break;
                                    } else {
                                        timeout_future.reset();
                                    }
                                }
                            },
                            shutdown_type = &mut stop_receiver => {
                                if let Ok(shutdown_type) = shutdown_type {
                                    handle_shutdown(shutdown_type);
                                    break;
                                }
                            },
                            shutdown_type = &mut shutdown => {
                                handle_shutdown(shutdown_type);
                                break;
                            },
                        }
                    }

                    for (_, aborter) in aborters.drain() {
                        let _ = aborter.send(());
                    }

                    return_value
                })
            }
        }

        pub use #ident as impl_produce;
    };
    output
}

fn extract_stream_item_type(impl_trait: &syn::TypeImplTrait) -> syn::Type {
    if impl_trait.bounds.len() == 1 {
        if let syn::TypeParamBound::Trait(bound) = &impl_trait.bounds[0] {
            if bound.path.segments.len() == 1 {
                let stream = &bound.path.segments[0];
                if stream.ident == "Stream" {
                    if let syn::PathArguments::AngleBracketed(arguments) = &stream.arguments {
                        if arguments.args.len() == 1 {
                            let argument = &arguments.args[0];
                            if let syn::GenericArgument::Binding(binding) = argument {
                                if binding.ident == "Item" {
                                    return binding.ty.clone();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    panic!("invalid stream request method return type");
}

fn modify_trait(mut item: syn::ItemTrait) -> syn::ItemTrait {
    if item.generics.lt_token.is_some() {
        panic!("generic traits are not supported");
    }

    item.items.push(
        syn::parse2::<syn::TraitItem>(quote! {
            /// Request arguments.
            type Request;
        })
        .unwrap(),
    );

    item.items.push(
        syn::parse2::<syn::TraitItem>(quote! {
            /// Transport error.
            type Error;
        })
        .unwrap(),
    );

    let must_use = syn::Attribute {
        pound_token: Token!(#)(Span::call_site()),
        style: syn::AttrStyle::Outer,
        bracket_token: syn::token::Bracket(Span::call_site()),
        path: syn::Path {
            leading_colon: None,
            segments: Punctuated::new(),
        },
        tokens: quote!(must_use),
    };

    for method in item.items.iter_mut().filter_map(|item| {
        if let syn::TraitItem::Method(func) = item {
            Some(func)
        } else {
            None
        }
    }) {
        if method.sig.asyncness.is_none() {
            panic!("all api methods should be marked as \"async\"")
        }

        if method.sig.generics.lt_token.is_some() {
            panic!("generic methods are not supported")
        }

        method.sig.asyncness = None;
        method.attrs.push(must_use.clone());

        match &method.sig.output {
            syn::ReturnType::Default => {
                method.sig.output = syn::parse2::<syn::ReturnType>(
                    quote!(-> zzrpc::ValueRequest<(), Request, Self::Error>),
                )
                .unwrap();
            }
            syn::ReturnType::Type(_, return_type) => match *return_type.to_owned() {
                syn::Type::ImplTrait(impl_trait) => {
                    let return_type = extract_stream_item_type(&impl_trait);
                    method.sig.output = syn::parse2::<syn::ReturnType>(
                        quote!(-> zzrpc::StreamRequest<#return_type, Request, Self::Error>),
                    )
                    .unwrap();
                }
                _ => {
                    method.sig.output = syn::parse2::<syn::ReturnType>(
                        quote!(-> zzrpc::ValueRequest<#return_type, Request, Self::Error>),
                    )
                    .unwrap();
                }
            },
        };
    }

    item
}

#[proc_macro_attribute]
pub fn api(_attr: TokenStream, item: TokenStream) -> TokenStream {
    if let Ok(item) = syn::parse2::<syn::ItemTrait>(item.into()) {
        let item_modified = modify_trait(item.clone());

        let request = request(&item);
        let response = response(&item);
        let consumer = consumer(&item);
        let impl_produce = impl_produce(&item);

        let output = quote! {
            #item_modified

            #request

            #response

            #consumer

            #impl_produce
        };

        output.into()
    } else {
        panic!("expected a trait")
    }
}

#[proc_macro_derive(Produce)]
pub fn produce(input: TokenStream) -> TokenStream {
    let syn::DeriveInput { ident, .. } = syn::parse_macro_input!(input);
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
            ) -> zzrpc::JoinHandle<zzrpc::ShutdownType> where
                Transport: mezzenger::Transport<
                        zzrpc::consumer::Message<Self::Request>,
                        zzrpc::producer::Message<Self::Response>,
                        Error,
                    > + mezzenger::Reliable
                    + mezzenger::Order
                    + Send
                    + 'static,
                Shutdown: zzrpc::futures::Future<Output = zzrpc::ShutdownType> + Send + 'static,
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
            ) -> zzrpc::JoinHandle<zzrpc::ShutdownType> where
                Transport: mezzenger::Transport<
                        zzrpc::consumer::Message<Self::Request>,
                        zzrpc::producer::Message<Self::Response>,
                        Error,
                    > + mezzenger::Reliable
                    + mezzenger::Order
                    + 'static,
                Shutdown: zzrpc::futures::Future<Output = zzrpc::ShutdownType> + 'static,
                SendErrorCallback: zzrpc::SendErrorCallback<Error> + 'static,
                ReceiveErrorCallback: zzrpc::ReceiveErrorCallback<Error> + 'static {
                impl_produce!(self, transport, configuration)
            }
        }
    };
    output.into()
}
