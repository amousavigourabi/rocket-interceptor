FROM ubuntu:22.04

RUN export LANGUAGE=C.UTF-8; export LANG=C.UTF-8; export LC_ALL=C.UTF-8; export DEBIAN_FRONTEND=noninteractive

COPY ./entrypoint.sh /entrypoint.sh

RUN mkdir -p /opt/ripple/bin
RUN mkdir -p /etc/opt/ripple/
COPY rippled /opt/ripple/bin

RUN ln -s /opt/ripple/bin/rippled /usr/bin/rippled

EXPOSE 80 443 5005 6005 6006 51235
ENTRYPOINT ["/entrypoint.sh"]
