@echo off

cargo "clean"
IF EXIST "xrpl-packet-interceptor.exe" (
    DEL  "xrpl-packet-interceptor.exe"
)
cargo "build" "--release"
COPY  "%CD%\target\release\xrpl-packet-interceptor.exe" "."