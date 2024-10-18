# Contributing to the Rocket Packet Interceptor project

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
export RUST_LOG=xrpl_packet_interceptor=info    # Linux
```

```powershell
$env:RUST_LOG = "xrpl_packet_interceptor=info"  # Windows PowerShell
```
