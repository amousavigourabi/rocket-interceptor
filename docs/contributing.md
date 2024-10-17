## How to contribute

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

```Linux
export RUST_LOG=xrpl_packet_interceptor=info    # Linux
```

```Windows Command Prompt
set RUST_LOG=xrpl_packet_interceptor=info       # Windows Command Prompt
```

```Windows PowerShell
$env:RUST_LOG = "xrpl_packet_interceptor=info"  # Windows PowerShell
```

## Testing

### Running tests

To run the tests you can use the following command:

```
cargo nextest run -E 'not (test(/grpc|docker/))'  
```

This will run all the tests except the integration tests that depend on grpc and docker.
You can use nextest [filter options](https://nexte.st/docs/filtersets/) to filter out the tests you do not want to run,
it is basically a regex
that matches the test module names.

#### gRPC tests

To run the grpc tests in `packet_client.rs` you need to have the controller running with `keep_action_log=False`,
you can configure this in the `__init__()` of a strategy by adding `keep_action_log=False,` to the `super().__init__()`.
You also need to configure your controller with `iteration_type=NoneIteration(timeout_seconds=300)`, this can be passed
as an argument to the strategy.

#### Docker tests

To run the `test_initialize_network` test in `docker_manager.rs` you need to have a docker engine running on your
machine in addition to the controller with the `NoneIteration` as IterationType.
If you want to run multiple integration tests that use shared resources (grpc client and docker engine), you need to run
them sequentially. You can do this by adding the `--test-threads=1` flag to the command.
So in short, to run all the tests you can run the following command with the controller and docker engine running on
your machine:

```
cargo nextest run --test-threads=1
```

### Generating testing reports

`llvm-cov` is used to generate coverage reports:

```
cargo install cargo-llvm-cov --locked       # This is only needed the first time
rustup component add llvm-tools-preview     # This is also only needed the first time
```

Branch coverage and excluding coverage are not available in stable rust, so you need to switch to nightly for that.
For this reason generating line coverage is explained firstly.
Then, generating coverage reports with branch coverage is explained.
After that, excluding coverage for tests is explained.
Lastly, running the manual tests with coverage is explained.

#### Line coverage

To run the tests in stable rust with coverage you can use the following command:

```
cargo llvm-cov nextest -E 'not (test(/integration/))' --open 
```

Notice: it is the same as running the tests without coverage, but with the `llvm-cov nextest` command instead
of `nextest run`. Refer to the section above for more information on running the tests (filter options and
prerequisites).
The --open flag will open the coverage report in your browser immediately after it is done for convenience. You can
remove this flag if you do not want this behavior. The coverage report will be saved in the `target/llvm-cov/html`
directory.

#### Branch coverage

To make a coverage report with branch coverage you can run the following:

```
rustup install nightly   # This is only needed the first time
cargo +nightly llvm-cov nextest -E 'not (test(/integration/))' --branch --open 
```

#### Exclude test code from coverage

By default, the coverage report includes the coverage of the tests. This generally not desired since the tests are not
part of the codebase.
In nightly there is the `#[coverage(off)]` coverage attribute that can be used to exclude the tests from the coverage
report. It does not completely work with `#[tokio::test]` tests to my knowledge, but it is still useful.
So to run the tests without the coverage of the tests you need to use nightly and put `#![feature(coverage_attribute)]`
at top of `main.rs`. And put `#[coverage(off)]` above the tests (or other functions) you want to exclude from the
coverage report.
For convenience these are already added as comments where necessary in the codebase. You can uncomment all of these and
run the tests as normal. For RustRover you can replace all the `// #[coverage(off)]` comments with `#[coverage(off)]` in
one go with ctrl+shift+R. Use the file mask `*.rs` to avoid replacing it in this README :).

#### Manual tests

For the manual tests you can first start the controller and the docker engine as explained
in [running-manually.md](running-manually.md) and then run the following command:

```
cargo llvm-cov run --branch --open
```

You need to make sure main will terminate without any errors to get the coverage report.
To do this you can wrap all the threads in a timeout and handle the error accordingly.
