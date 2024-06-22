#!/bin/bash

cargo clean
rm xrpl-packet-interceptor
cargo build --release
cp ./target/release/xrpl-packet-interceptor .
