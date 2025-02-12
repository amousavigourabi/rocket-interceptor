#!/bin/bash

cargo clean
rm rocket-interceptor
cargo build --release
cp ./target/release/rocket-interceptor .
