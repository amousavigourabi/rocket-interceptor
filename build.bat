@echo off

cargo "clean"
IF EXIST "rocket-interceptor.exe" (
    DEL  "rocket-interceptor.exe"
)
cargo "build" "--release"
COPY  "%CD%\target\release\rocket-interceptor.exe" "."