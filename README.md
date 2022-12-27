# zzrpc

[RPC](https://en.wikipedia.org/wiki/Remote_procedure_call) over [mezzenger](https://github.com/zduny/mezzenger) transports.

https://crates.io/crates/zzrpc

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/O5O31JYZ4)

# usage

See [zzrpc-tutorial](https://github.com/zduny/zzrpc-tutorial).

# targeting WebAssembly

See [rust-webapp-template-api](https://github.com/zduny/rust-webapp-template-api).

# further work

Following improvements are planned for development:

1. Support for two more method types:

  - method with default return without acknowledgment - its future will return as soon as request message is sent to producer without waiting for `()` response from the producer:
    ```rust
    #[no-ack]
    async fn do_something_i_dont_care_if_it_completes(&self, some_argument: i32);
    ```
 
  - method with [`CancellationToken`](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html) argument - so producer implementors can receive request abort messages (currently "aborting" a request means only that the producer will ignore task's result and not send it to the consumer - it doesn't mean the task itself is meaningfully affected):
    ```rust
    use tokio_util::sync::CancellationToken;
  
    // ...
  
    async fn do_some_task(&self, cancellation_token: CancellationToken, some_argument: i32) -> u64;
    ```

2. An option to generate bindings so consumer methods could be called directly from JavaScript using [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen).

# see also

[mezzenger](https://github.com/zduny/mezzenger)

[remote procedure call](https://en.wikipedia.org/wiki/Remote_procedure_call)
