#!/usr/bin/env bash
# lucy and lucy-indicator must never link audio or ML crates (ADR 0006).
set -euo pipefail

forbidden='^(whisper-rs|whisper-rs-sys|webrtc-audio-processing|ort|ort-sys|cpal|tract-onnx|livekit-wakeword) '
status=0
for crate in lucy lucy-indicator; do
    if found=$(cargo tree --locked -p "$crate" -e normal,build --prefix none | grep -E "$forbidden"); then
        echo "$crate depends on audio/ML crates:" >&2
        echo "$found" | sort -u >&2
        status=1
    fi
done
exit $status
