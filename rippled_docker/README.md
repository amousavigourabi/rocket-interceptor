# Building a patched Ripple Daemon Docker image

## Pre-requisites

- The [rippled](https://github.com/XRPLF/rippled/tree/develop) source code
- [Requirements](https://github.com/XRPLF/rippled/blob/develop/BUILD.md) for building `rippled`
- A Linux environment (warning: this was only tested with Ubuntu 22.04)

## Patching the code

The piece of code that needs to be removed is located in [overlay/impl/Handshake.cpp](https://github.com/XRPLF/rippled/blob/2.2.0/src/ripple/overlay/impl/Handshake.cpp#L319), on lines 319-321.
The code consists of checking the output of a call to `verifyDigest`, which verifies the `Session-Signature` header when requesting a Peer Connection, and throws a critical error when the signature is invalid. 

```cpp
if (!verifyDigest(publicKey, sharedValue, makeSlice(sig), false)) // Both of these lines
            throw std::runtime_error("Failed to verify session"); // should be removed.
```

These 2 lines can be safely deleted, after which the program can be compiled. Compilation instructions can be found [here](https://github.com/XRPLF/rippled/blob/develop/BUILD.md).

## Creating the Docker Image

After successfully building the `rippled` executable, it needs to be placed in this directory. 
After which, you can run `docker build -f rippled_patched.Dockerfile . -t <tag>`

This command creates a Docker Image that is almost identical to the official XRPL Docker Image seen [here](https://hub.docker.com/repository/docker/isvanloon/rippled-no-sig-check/general).

## Using the Docker Image

Since the interceptor will automatically look for updates of the created image on Docker Hub, your image should be pushed to Docker Hub.
After pushing, you need to navigate to `src/docker_manager.rs` and replace the global variable `IMAGE` with the full name of your pushed Docker Image.

After this, the new Docker image is ready to be used, and you can rebuild the interceptor application using `cargo build --release`.

Disclaimer: There is a possibility the interceptor breaks after a breaking change to the XRP Ledger Peer protocol, `rippled` versions after 2.1.1 are not guaranteed to work.
