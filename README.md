# zzrpc

[RPC](https://en.wikipedia.org/wiki/Remote_procedure_call) over [mezzenger](https://github.com/zduny/mezzenger) transports.

https://crates.io/crates/zzrpc

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/O5O31JYZ4)

# usage

## Step 0 (optional) - `common` crate

Create separate `common` crate for definition of your api.

This step is optional, but doing so makes it easier to share code between client and server 
implementations, especially when they're targeting different platforms (for example: server 
targeting native platform and client targeting WebAssembly running in a browser).

```bash
cargo new --lib common
```

## Step 1 - dependencies

Add [`zzrpc`](https://crates.io/crates/zzrpc) to `Cargo.toml`:

```toml
# ...

[dependencies]
# ...
zzrpc = "0.1.1"
```

Most likely (if you want to use complex types as arguments or return values) you'll also
need [`serde`](https://crates.io/crates/serde):

```toml
# ...

[dependencies]
# ...
zzrpc = "0.1.1"
serde = { version = "1.0.150", features = ["derive"] }
```

You'll also need [mezzenger](https://crates.io/crates/mezzenger) and on native
server/client [tokio](https://crates.io/crates/tokio):

```toml
# ...

[dependencies]
# ...
zzrpc = "0.1.1"
mezzenger = "0.1.2"
serde = { version = "1.0.150", features = ["derive"] }
tokio = { version = "1.23.0", features = ["full"] }
tokio-stream = "0.1.11" # optional but useful when creating stream responses 
```

If you followed **Step 0** then add `common` to your client/server crate dependencies:

```toml
# ...

[dependencies]
# ...
common = { path = "../common" }
zzrpc = "0.1.1"
mezzenger = "0.1.2"
serde = { version = "1.0.150", features = ["derive"] }
tokio = { version = "1.23.0", features = ["full"] }
tokio-stream = "0.1.11" # optional but useful when creating stream responses
```

## Limitations

Have in mind following limitations of `zzrpc`'s `#[api]` macro:
- generic traits are not supported,
- generic methods are not supported,
- only one api is allowed per module 
  (but multiple apis in separate modules of the same crate are fine),
- only one producer implementation is allowed per module
  (again: if you need more you can simply define them in separate modules).

# see also

[mezzenger](https://github.com/zduny/mezzenger)

[remote procedure call](https://en.wikipedia.org/wiki/Remote_procedure_call)
