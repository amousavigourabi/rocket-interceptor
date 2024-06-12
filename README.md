# XRPL packet interceptor



## Getting started

See the related repository [xrpl-controller-module](https://gitlab.ewi.tudelft.nl/cse2000-software-project/2023-2024/cluster-q/13d/xrpl-controller-module) for the controller.

### Prerequisites
- [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- You need to download protoc to be able to build. This will generate the necessary files for the proto.file during the build process. To install this on your machine please refer to [this](https://github.com/hyperium/tonic?tab=readme-ov-file#dependencies)

## How to run
1. Clone this repository and the controller repository
2. Make a strategy in the controller and pass it as parameter to the serve method in main
3. Configure the ports and amount of validator nodes to use in the interceptor in the `network-config.toml` file
4. Run the controller
5. Run the interceptor

Below is a guide on how to run the packet interceptor. Refer to the controller repository for instructions on how to run the controller.
```
cargo run
```

## How to contribute
1. Create a new branch from `main`
2. Make your changes
3. Make sure the pipeline passes
4. Push your branch to the repository
5. Create a merge request to `main`

### The pipeline
- Building the application with `cargo`
- Checking the code style with `rustfmt`
- Checking for common code flaws with `clippy`
- Testing with `cargo`
- Generating docs with `cargo doc`

To replicate the pipeline on your own machine, you can just simply run the commands specified in the gitlab-ci.yml file.

To run nextest you need to download the nextest binary from [here](https://nexte.st/book/pre-built-binaries)

### gRPC
If you want to make changes to a .proto file it is important that you make the exact same changes in the corresponding file in the interceptor and regenerate the necessary files both in the controller and interceptor.
- For the controller run:
```
python -m grpc_tools.protoc -I. --python_out=. --grpc_python_out=. protos/packet.proto
```
- For the interceptor you can just rebuild.

### Logging
For terminal logging we use env_logger. You can set the log level by setting the `RUST_LOG` environment variable. The levels are `trace`, `debug`, `info`, `warn`, `error`.

For file logging we created a custom logger. You can create a file with a file type you specify with `create_log`. It will be kept under the `logs/[start_time]/` directory. 
Initialize it in the lazy_static block. Then you can log to the file by calling the `log!` macro, which needs the file and string to log.

### Generating testing reports
To make a coverage report you can run the following:
```
cargo install cargo-llvm-cov --locked   # This is only needed the first time
rustup default nightly                  # switch to nightly since the branch coverage is not available in stable
cargo llvm-cov nextest -E 'not (test(/unit*|integration*|fail*/))' --branch --open 
```
You can run it with `unit*` `integration*` or `fail*` in the regex to filter out the tests you want to include in the report.

For the manual tests you can first start the controller and then run the following command:
```
cargo llvm-cov run --branch --open
```
You need to make sure main will terminate without any errors to get the coverage report. 
To do this you can wrap all the threads in a timeout and handle the error accordingly.