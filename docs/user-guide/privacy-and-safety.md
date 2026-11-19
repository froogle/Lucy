# Privacy and safety

Lucy listens all the time and can act on your computer, so it's worth knowing exactly what she does with what she hears and what she asks before she acts.

## What stays on your machine

- **The microphone.** Lucy listens continuously for the Wake word, on your machine. Nothing is recorded or sent anywhere until you say it.
- **Speech-to-text.** With Whisper (the default), your speech is turned into text on your machine. With Superwhisper, it goes through Superwhisper, which keeps a history of what you said for as long as its own settings say.
- **Her voice.** Piper and Kokoro run on your machine.
- **Secrets you type.** Passwords and API keys typed into an Input dialog for a Plugin go to your keyring and to that Plugin. They never reach the thinking model, the log or Lucy's public socket.

## What leaves your machine

- **What you say in a Session**, as text, goes to the thinking Provider you chose (Anthropic for Claude, OpenAI for Codex), along with Lucy's replies and the results of the tools she uses. This is the same as typing it into Claude Code or Codex yourself, and their own privacy terms apply.
- **Screenshots**, when Lucy looks at your screen, go to the same Provider.
- **What Plugins fetch.** The news Plugin reads news feeds; other Plugins reach the hosts their Manifest declares.

With Claude, Lucy deletes the Session's conversation from disk when the Session ends.

## What Lucy asks before acting

Lucy's runtime, not the thinking model, decides when to ask, and hears your answer itself. A model can't say yes on your behalf.

| Before | She asks | And |
|---|---|---|
| Using a new or changed Plugin | for your **Approval**, reading out what it can do and reach | only by voice; any change needs a fresh yes |
| A risky Plugin action (force-quitting an app, anything its author marked) | for a **Confirmation** | each time |
| Looking at or using your screen | "I need to look at and use your screen. That lasts until Lucy restarts. Okay?" | once, until Lucy restarts |
| Anything your own Claude asks permission for during a Hand-off | "Claude wants to …. Okay?" | each time |
| Removing a Plugin | for a Confirmation | each time |

Only a clear "yes" counts. Anything else, or silence, is a no.

Some things never need a yes: opening a web page, starting an app, finding or opening your files, telling you about your system, changing her Persona, and changing your windows or theme through the Omarchy Plugin. They act only on your own desktop session.

## Your screen

Your yes to the screen lasts until Lucy restarts, and covers looking, clicking, typing, pressing keys and scrolling. After it, she doesn't ask again before each click.

That's convenient, but bear in mind that what's on screen is untrusted: a web page could contain text written to trick the model into clicking "Buy" or "Send". While you've said yes, nothing stops that click. If that worries you, say no, restart Lucy (`lucy service restart`) when you've finished with the screen, or turn screen access off with `[screen] enabled = false`.

Lucy refuses to type into a password field, and asks you to type it yourself.

## Plugins

Every Plugin runs in a Landlock sandbox. It can read its own folder and the system's programs, write only its own private folder, and reach anything else only if its Manifest says so and you approved it. Network access is declared and read out to you, but not yet enforced. See [Plugins](plugins.md).

The shipped Omarchy Plugin starts the screenshot tool and the night light through Hyprland, which runs them outside its sandbox.

## Hand-offs

A Hand-off runs your own Claude, unsandboxed, with your own permission rules. Anything you've already allowed in your Claude settings runs without Lucy asking. See [Handing work to Claude](hand-off.md#what-claude-may-do).

## Billing

Lucy is built to run on subscriptions you already pay for. Unless you set `allow_api_key = true` for a Provider, Lucy removes API-key variables from `claude`'s environment, and refuses to use a `claude` that would bill an API key.
