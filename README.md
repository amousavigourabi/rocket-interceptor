# XRPL packet interceptor



## Getting started

See the related repository [xrpl-controller-module](https://gitlab.ewi.tudelft.nl/cse2000-software-project/2023-2024/cluster-q/13d/xrpl-controller-module) for the controller.

### Prerequisites
- [cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- You need to download protoc to be able to build. This will generate the necessary files for the proto.file during the build process. To install this on your machine please refer to [this](https://github.com/hyperium/tonic?tab=readme-ov-file#dependencies)

## The pipeline
- Building the application with `cargo`
- Checking the code style with `rustfmt`
- Checking for common code flaws with `clippy`
- Testing with `cargo`
- Generating docs with `cargo doc`

To replicate the pipeline on your own machine, you can just simply run the commands specified in the gitlab-ci.yml file.

To run nextest you need to download the nextest binary from [here](https://nexte.st/book/pre-built-binaries)

## How to contribute
1. Create a new branch from `main`
2. Make your changes
3. Push your branch to the repository
4. Create a merge request to `main`

### gRPC
If you want to make changes to a .proto file it is important that you make the exact same changes in the corresponding file in the interceptor and regenerate the necessary files both in the controller and interceptor.
- For the controller run:
```
python -m grpc_tools.protoc -I. --python_out=. --grpc_python_out=. protos/packet.proto
```
- For the interceptor you can just rebuild.