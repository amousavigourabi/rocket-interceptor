# XRPL packet interceptor

## How to build

### Prerequisites
- [rust and cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- [protoc](https://github.com/hyperium/tonic?tab=readme-ov-file#dependencies)
- OpenSSL
- Docker (for Windows and macOS: make sure Docker Engine is active by launching Docker Desktop)

### Running the build
```console
cargo build --release
```

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