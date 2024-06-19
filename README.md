# XRPL packet interceptor

## How to build

### Prerequisites
- [rust and cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- [protoc](https://github.com/hyperium/tonic?tab=readme-ov-file#dependencies)
- OpenSSL
- Docker (for Windows and macOS: make sure Docker Engine is active by launching Docker Desktop)

### Building the executable

```console
cargo build --release
```

The executable can be found in the folder `target/release`.

## The Ripple daemon Docker image

For the interceptor to work, we have made a slight modification to the official [rippled](https://github.com/XRPLF/rippled) Docker [image](https://hub.docker.com/r/xrpllabsofficial/xrpld).
The Docker image we used to develop the interceptor is based on `rippled` version `2.1.1`. In this custom docker image we have removed a [signature check](https://github.com/XRPLF/rippled/tree/2.1.1/src/ripple/overlay#session-signature) when instantiating a connection to a peer.
The removal of this check was needed since we use a MITM (Man-In-The-Middle) approach for intercepting the network's messages, which the check is trying to prevent. 

For your convenience we have uploaded a Docker image containing the patched `rippled v2.1.1` program to [Docker Hub](https://hub.docker.com/repository/docker/isvanloon/rippled-no-sig-check/general).
Instructions to patch this yourself for future updates to the `rippled` program are included in the README of the `rippled_docker/` subdirectory.

## How to contribute
1. Create a new branch from `main`
2. Make your changes
3. Make sure the pipeline passes locally
4. Push your branch to the repository
5. Create a merge request to `main`

### The pipeline
- Building the application with `cargo`
- Checking the code style with `rustfmt`
- Checking for common code flaws with `clippy`
- Testing with `cargo nextest`
- Generating docs with `cargo doc`

To replicate the pipeline on your own machine, you can just simply run the commands specified in the `gitlab-ci.yml` file.

For `cargo nextest run` to work, you need to download the `nextest` binary from [here](https://nexte.st/book/pre-built-binaries)

### gRPC
If you want to make changes to a .proto file it is important that you make the exact same changes in the corresponding file in the interceptor and regenerate the necessary files both in the controller and interceptor.

For the interceptor, the .proto files are automatically recompiled when the application is built.

### Logging
For terminal logging we use env_logger. You can set the log level by setting the `RUST_LOG` environment variable. The levels are `trace`, `debug`, `info`, `warn`, `error`.

For file logging we created a custom logger. You can create a file with a file type you specify with `create_log`. It will be kept under the `logs/[start_time]/` directory. 
Initialize it in the lazy_static block. Then you can log to the file by calling the `log!` macro, which needs the file and string to log.