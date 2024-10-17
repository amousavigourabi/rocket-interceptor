# Rocket's packet interceptor

This module is part of Rocket and is responsible for initializing the XRPL validator nodes and intercepting
all the messages
sent over the network while executing actions determined by the controller. This module is run as a subprocess of the
controller. So it is not supposed to run as a standalone application.

## Prerequisites

- [rust and cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html)
- [protoc](https://github.com/hyperium/tonic?tab=readme-ov-file#dependencies)
- OpenSSL (read further for install guide)
- Docker (for Windows and macOS: make sure Docker Engine is active by launching Docker Desktop)

### OpenSSL

To install OpenSSL, see the official installation guide at https://github.com/openssl/openssl.
Although this is the official guide, there are alternatives that may be easier for you.
Note that this worked on our machines, but we cannot guarantee that this works for every machine.
We recommend Windows users to use WSL with a Debian based distro as it is easier to install OpenSSL on it.

**Windows**

Note that this is for a 64 bit machine running on an Intel chip with the x86_64 architecture. For other architectures
this may be a bit different.
For Windows, it is possible to download .exe/.msi installers via https://slproweb.com/products/Win32OpenSSL.html.
After installation, you should set two environment variables:

* OPENSSL_DIR = path_to\OpenSSL-Win64
* OPENSSL_LIB_DIR = path_to\OpenSSL-Win64\lib\VC\x64\MTd

**Linux**

For Debian based distro's, it is possible to use 'apt'.

```console
sudo apt install openssl libssl-dev pkg-config
```

**MacOS**

For MacOS, it is possible to use HomeBrew.

```console
brew install openssl
```

### Building the executable

**Linux/macOS**

```console
./build.sh
```

**Windows**

```console
.\build.bat
```

After executing the build script, the executable file should now be available in the root of this repository.

## Useful resources

- If you want to contribute read: [contributing.md](docs/running-manually.md)
- If you want to run the interceptor manually read: [running-manually.md](docs/running-manually.md)
- Document about modification to the rippled Docker image: [rippled-docker-image.md](docs/rippled-docker-image.md)