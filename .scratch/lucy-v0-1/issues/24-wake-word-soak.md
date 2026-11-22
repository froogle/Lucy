# Does the wake-word engine hold up over hours of normal desktop audio?

Type: task
Status: moved to GitHub [#2](https://github.com/froogle/Lucy/issues/2)
Blocked by: 16, 27

## Question

Run `livekit-wakeword` with the chosen wake phrase's model over a few hours of normal desktop audio (music, video, calls). Record CPU, memory and false accepts, with and without the echo-cancellation chain in front of it. Feeds the default sensitivity, and whether AEC must run before wake-word detection.
