@echo off

cargo "clean"
DEL  "xrpl-packet-interceptor"
cargo "build" "--release"
COPY  "%CD%\target\release\xrpl-packet-interceptor" "."