# Contributing to the Rocket Interceptor

The document here aims to aid developers in understanding 
how to contribute to the controller module. Before making any changes, 
read this file carefully. Below, you will find the common process of contributing to the project.

1. Create a new branch from `main`
2. Make your changes
3. Make sure the pipeline passes locally
4. Push your branch to the repository
5. Create a merge request to `main`

## The pipeline

- Building the application with `cargo`
- Checking the code style with `rustfmt`
- Checking for common code flaws with `clippy`
- Testing with `cargo nextest`
- Generating docs with `cargo doc`

To replicate the pipeline on your own machine, you can just simply run the commands specified in the `gitlab-ci.yml`
file.

For `cargo nextest run` to work, you need to download the `nextest` binary
from [here](https://nexte.st/book/pre-built-binaries)

## gRPC

If you want to make changes to a .proto file it is important that you make the exact same changes in the corresponding
file in the controller and regenerate the necessary files both in the controller and interceptor.

For the interceptor, the .proto files are automatically recompiled when the application is built.

## Logging

For terminal logging we use env_logger. You can configure the log level by setting the `RUST_LOG` environment variable
before running. The levels are `trace`, `debug`, `info`, `warn`, `error`.
For example to set the log level to info you can execute the following command:

```bash
export RUST_LOG=rocket_interceptor=info
```

```powershell
$env:RUST_LOG = "rocket_interceptor=info"
```

## Running the interceptor manually

This guide will show you how to run the interceptor manually. This is useful for debugging.
In order to run the interceptor manually, ensure the following:

1. the Controller Module (gRPC server) is running
2. (for Windows & Mac) Docker Engine is running on your machine

After you have these prerequisites, you can run the interceptor manually with `cargo run`.

## Running the gRPC server

This is in the controller module. You do have to run it with the NoneIteration as IterationType.
Since this the one that does not run the interceptor as a subprocess.
There are some other configuration options in the controller that determine how the interceptor operates such as the
network configuration or the strategy configuration.
Read
the [controller's README](https://github.com/diseb-lab/rocket)
for more information on how to configure these.
