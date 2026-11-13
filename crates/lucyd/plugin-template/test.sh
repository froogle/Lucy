#!/bin/sh
# Runs the Plugin's tests. They use recorded responses, never the network.
set -e
cargo test --release
