"""Lucy's Kokoro server, run by lucyd in the venv that holds kokoro-onnx.

POST /synthesize with {"text": ..., "voice": ...} answers a 16-bit mono WAV,
as Piper's http_server does. "voice" may be left out for the --voice the
server started with.
"""

import argparse
import io
import json
import wave
from http.server import BaseHTTPRequestHandler, HTTPServer

import numpy as np
from kokoro_onnx import Kokoro

LANGUAGES = {"a": "en-us", "b": "en-gb"}


def to_wav(samples, sample_rate):
    pcm = (np.clip(samples, -1.0, 1.0) * 32767).astype("<i2")
    out = io.BytesIO()
    with wave.open(out, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate)
        wav.writeframes(pcm.tobytes())
    return out.getvalue()


def main():
    parser = argparse.ArgumentParser()
    for flag in ("--host", "--model", "--voices", "--voice"):
        parser.add_argument(flag, required=True)
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    kokoro = Kokoro(args.model, args.voices)
    voices = set(kokoro.get_voices())

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            if self.path != "/synthesize":
                self.send_error(404)
                return
            length = int(self.headers.get("Content-Length", 0))
            request = json.loads(self.rfile.read(length))
            voice = request.get("voice") or args.voice
            if voice not in voices:
                self.send_error(400, f"{voice} isn't a Kokoro voice")
                return
            language = LANGUAGES.get(voice[0], "en-us")
            samples, sample_rate = kokoro.create(request["text"], voice=voice, lang=language)
            body = to_wav(samples, sample_rate)
            self.send_response(200)
            self.send_header("Content-Type", "audio/wav")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    HTTPServer((args.host, args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
