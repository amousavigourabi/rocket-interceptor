## Running the interceptor manually

This guide will show you how to run the interceptor manually. This is mainly useful for testing purposes.
In order to run the interceptor manually, you need to have:

1. the gRPC server running
2. docker engine running on your machine

After you have these prerequisites, you can run the interceptor manually with `cargo run`.

### Running the gRPC server

This is in the controller module. You do have to run it with the NoneIteration as IterationType.
Since this the one that does not run the interceptor as a subprocess.
There are some other configuration options in the controller that determine how the interceptor operates such as the
network configuration or the strategy configuration.
Read
the [controller's README](https://gitlab.ewi.tudelft.nl/cse2000-software-project/2023-2024/cluster-q/13d/xrpl-controller-module/-/blob/main/README.md?ref_type=heads)
for more information on how to configure these.

### Running Docker Engine

For windows and macOS, make sure Docker Engine is active by launching Docker Desktop.
For Linux docker engine is usually running by default (on system startup).