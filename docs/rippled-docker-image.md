## The Ripple daemon Docker image

For the interceptor to work, we have made a slight modification to the
official [rippled](https://github.com/XRPLF/rippled) Docker [image](https://hub.docker.com/r/xrpllabsofficial/xrpld).
The Docker image we used to develop the interceptor is based on `rippled` version `2.1.1`. In this custom docker image
we have removed a [signature check](https://github.com/XRPLF/rippled/tree/2.1.1/src/ripple/overlay#session-signature)
when instantiating a connection to a peer.
The removal of this check was needed since we use a MITM (Man-In-The-Middle) approach for intercepting the network's
messages, which the check is trying to prevent.

For your convenience we have uploaded a Docker image containing the patched `rippled v2.1.1` program
to [Docker Hub](https://hub.docker.com/repository/docker/isvanloon/rippled-no-sig-check/general).
Instructions to patch this yourself for future updates to the `rippled` program are included in the README of
the `rippled_docker/` subdirectory.