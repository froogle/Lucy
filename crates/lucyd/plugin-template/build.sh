#!/bin/sh
# Builds the Plugin. lucyd runs this, then test.sh, on submit_plugin.
set -e
cargo build --release
