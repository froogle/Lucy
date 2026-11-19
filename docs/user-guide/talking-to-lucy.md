# Talking to Lucy

## Opening a Session

Say **"Hey Lucy"** and carry straight on with your request, or pause and wait for the Indicator. Either works: whatever you say after the Wake word belongs to the Session it opens.

You can also open a Session without speaking, with `lucy listen` (handy for a keyboard shortcut).

## Talking

Talk normally. Lucy waits until you've paused for about three-quarters of a second, then works on what you said. While she thinks, the Indicator says so; then she answers out loud, a sentence at a time.

You don't need to say "Hey Lucy" again while the Session is open: just keep talking.

### Cutting in

If Lucy is talking and you want to say something, just start speaking. She stops, turns herself down and listens. This is called **Barge-in**.

- Barge-in by speech works best on PipeWire. On PulseAudio or ALSA it is off by default, because Lucy's echo cancelling works less well there; say "Hey Lucy" to cut in instead.
- If Lucy keeps cutting herself off because she hears her own voice through your speakers, set `[barge_in] mode = "wake_word"` (see [Settings](settings.md#barge_in)), or use headphones.

## Ending a Session

Any of these ends it:

- Say something that means you're done: "that's all, thanks", "bye Lucy", "that'll do". Lucy understands what you mean; there's no fixed list.
- Say nothing for 20 seconds.
- Click the Indicator, or the **End** button on her notification.
- Run `lucy end`.

## What Lucy can do

Out of the box, Lucy can:

| Ask | Example |
|---|---|
| Open a web page | "Open the BBC weather page." |
| Start an app | "Open Firefox." "Start a terminal." |
| Find, show and open your files | "Find my tax return." "Show me the holiday photos." "Open the budget spreadsheet." |
| Tell you about your computer | "How's my battery?" "How much disk space have I got?" "What time is it?" |
| Read you the news | "What's the news?" "Give me a detailed summary." "Add the Guardian." |
| Manage windows and workspaces (Omarchy) | "Move Firefox to workspace 3." "Close the terminal." "Switch to tiling." |
| Change Omarchy's settings | "Switch to the Tokyo Night theme." "Hide the bar." "Turn on night light." "Lock the screen." |
| Take a screenshot | "Take a screenshot." |
| Look at and use your screen | "What does this error say?" "Play the third video." "Click Sign in." |
| Pass work to your own Claude | "Tell Claude to fix the failing test in my lucy folder." |
| Build herself a new ability | "Can you sort my downloads folder by type?" (when no Plugin can) |

Lucy's general knowledge comes from the thinking model, so you can also just ask her things and talk things through.

`find_files` searches your home folder, skipping hidden folders and the usual clutter (`node_modules`, `.git` and the like). [Settings](settings.md#files) can change where she looks.

### The news

The news Plugin reads official news feeds and summarises them for you. It only summarises Sources whose terms allow it; for the others, Lucy offers to open the site's own page instead. Tell her which Sources and topics you like and how long you want the briefing, and she remembers.

### Your screen

Lucy can look at your screen and act on it: click, type, press keys and scroll. The first time she needs to after starting, she asks:

> "I need to look at and use your screen. That lasts until Lucy restarts. Okay?"

Your yes lasts until Lucy restarts (`lucy service restart`, or logging out). While she's looking, the Indicator shows a mark.

Lucy won't type into a password field. If a page asks for a password, she asks you to type it yourself, in an Input dialog.

Screenshots go to the thinking model, so they leave your machine. To turn screen access off altogether, set `[screen] enabled = false`. See [Privacy and safety](privacy-and-safety.md#your-screen).

## Saying yes: Confirmations

Before Lucy does something risky (force-quitting an app, removing a Plugin, or anything a Plugin's author marked as needing a check), she asks you first, out loud, for example:

> "Shall I force-quit that window (Slack)? Anything unsaved in it will be lost."

This is a **Confirmation**. Lucy hears your answer herself; the thinking model never decides whether you said yes.

- Say **"yes"**, "yeah", "yep", "go ahead" or "do it". Your whole reply must be one of these.
- Anything else, including "yes but…", a question, or 8 seconds of silence, means **no**.

You can change the yes-phrases and the wait in [Settings](settings.md#confirmation).

## Typing instead of talking: the Input dialog

Some things shouldn't be said out loud, or are hard to say: a password, an API key, a long web address. When Lucy needs one, she says so and opens an **Input dialog**: a box on screen to type in.

- On Omarchy it opens in the bar panel. Elsewhere it opens in a terminal window.
- What you type for a secret goes only to the Plugin that asked, and into your keyring. It never reaches the thinking model.
- To cancel, press Esc, or say "cancel", "never mind" or "forget it".
- The box closes on its own after two minutes.

## Long jobs

Some requests take minutes: building a Plugin, or a [Hand-off](hand-off.md) to your own Claude. These carry on after the Session ends. Lucy tells you when they're done, in a notification and again at the start of your next Session. If she needs to ask you something meanwhile, she waits until you're talking to her.

To stop one, say **"cancel it"**. If both a build and a Hand-off are running, Lucy asks which.

## Her Persona

Lucy's **Persona** is her personality and manner, and what she calls you. Shape it by telling her:

- "Call me Pete."
- "Keep your answers shorter."
- "You can be a bit more sarcastic."

She saves it in `~/.config/lucy/persona.md`, a plain text file you can edit yourself. Delete it and she introduces herself afresh next time.
