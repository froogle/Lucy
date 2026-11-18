# Lucy v0.2: Hand-off and the screen

Status: ready-for-agent

Synthesised from a grilling session on build tickets 34, 35, 38 and 41 (2026-11-16), the [v0.1 spec](../lucy-v0-1/spec.md) as amended that day, `GLOSSARY.md` and ADRs 0001–0008 (0007 and 0008 are new). Vocabulary follows the glossary, including the new terms **Hand-off** and **Input dialog**. Building starts once v0.1 acceptance (build ticket 24) passes. Tickets 01–05, 11 and 12 are the Hand-off, 06, 07, 10, 13 and 14 the screen, 08 finding files and 09 a Delegate fix. Slices: 1 is 09 (first, as it blocks every Authoring build), 01, 06 and 08; 2 is 02, 03, 04, 05, 07 and 11; 3 is 10 and 12; 4 is 13 and 14. 03 reuses 08's walker. Amended by a second grilling session (2026-11-17); 04 also needs v0.1's Input dialog (build ticket 34), and 06 needs the Omarchy Plugin (build ticket 35).

## Problem Statement

Lucy refuses two kinds of request the user keeps making.

The first is "tell Claude to …". The user has Claude Code set up with their own folders, settings and integrations (Linear, email and others over MCP). They want to pass a job to it by voice: "tell Claude to fix the failing test in lucy", "tell Claude to grab my Linear tickets". Lucy says she can't. Her own `claude` is deliberately isolated: no user config, no built-in tools, only her own MCP server. Her Authoring Delegate is sandboxed away from the user's data. Raw shell access is out of scope.

The second is anything that needs her to see the screen. She can open YouTube and search, but "now play the third video" does nothing, because she can't see what's there or click it. That holds for every app, not just the browser.

## Solution

**Hand-off.** "Tell Claude to …" becomes a Hand-off: a new kind of Delegate that runs the user's own `claude`, with their own config, MCP servers and permission rules, in the folder they name (or their home folder). Anything their Claude settings already allow runs. Anything else, Claude asks for, and Lucy turns that into a spoken Confirmation she hears herself, with the details on screen. She keeps the Session open while it works, raises Claude's questions aloud (or in the Input dialog when the answer must be typed) and speaks a short summary at the end, with the full answer in a notification. If the Session has ended, the result waits for the next one. The transcript is kept so the user can resume it at a keyboard.

**Seeing and acting on the screen.** The desktop Plugin (the Omarchy Plugin first) gains screen tools: take a screenshot for the model, click, type, press keys, scroll. The Session model, which already reads images, looks, acts and looks again to check. The first time she uses the screen after lucyd starts, she asks for the user's spoken yes, because the image leaves the machine and she will act on it; the yes lasts until lucyd restarts. She never types into a password field; secrets go through the Input dialog.

## User Stories

### Hand-off

1. As a user, I want to say "tell Claude to fix the failing test in lucy", so that I can pass coding work to Claude Code without going to a keyboard.
2. As a user, I want to say "tell Claude to grab my Linear tickets" or "check my email", so that I can use the integrations I've connected to Claude by voice.
3. As a user with Claude integrations, I want the Hand-off to run with my own Claude config and MCP servers, so that everything I've set up is there.
4. As a user, I want a folder I name loosely ("in lucy") found for me, so that I don't have to say full paths.
5. As a user, I want Lucy to ask which one I mean when several folders match, and say so when none does, so that the work lands in the right place.
6. As a user, I want an absolute path or a `~` path used exactly as I say it, so that I can be precise when I need to.
7. As a user, I want a request that names no folder to run in my home folder, so that integration-only requests just work.
8. As a user, I want what my Claude settings already allow to run without asking, so that the Hand-off is as smooth as using Claude myself.
9. As a user, I want anything else Claude wants to do spoken to me as "Claude wants to … Okay?", and heard by Lucy herself, so that nothing runs without my yes.
10. As a user, I want the full tool name and input in a notification when Claude asks, so that I can check exactly what I'm agreeing to.
11. As a user, I want silence or anything but a clear yes to deny the request, so that the safe outcome is the default.
12. As a user, I want the hand-off itself to start without a Confirmation, so that asking for it is enough.
13. As a user, I want the Session to stay open with a "working" mark on the Indicator while Claude works, so that I can ask other things meanwhile.
14. As a user, I want Claude's questions asked aloud when nobody is speaking, so that it can check things with me as it goes.
15. As a user, I want Claude's questions that need a typed answer to open the Input dialog, so that URLs and secrets don't go through speech.
16. As a user, I want a one-to-three-sentence spoken summary when Claude finishes, so that I hear the outcome.
17. As a user, I want Claude's full final answer in a notification, so that I can read the details.
18. As a user, I want a Hand-off to carry on if the Session ends, with a notification when it's done and the summary offered at my next Session, so that I don't have to wait around.
19. As a user, I want a question Claude asks after the Session ended raised at my next Session, so that the work can continue.
20. As a user, I want the final notification to give me the `claude --resume` command and the folder, so that I can pick the work up at my keyboard.
21. As a user, I want the Hand-off transcript kept, so that I can see what Claude did.
22. As a user, I want "cancel it" to stop a Hand-off, and Lucy to ask which one I mean if an Authoring build is also running, so that I can call it off.
23. As a user, I want a Hand-off stopped after 60 minutes (configurable), with the resume command sent to me, so that a stuck one doesn't run forever.
24. As a user, I want a second "tell Claude" request while one is running answered with "Claude's still on the lucy test; want me to queue this?", so that hand-offs don't collide.
25. As a user, I want a Hand-off and an Authoring build able to run at the same time, so that one doesn't block the other.
26. As a user, I want the Hand-off to use my Claude's default model, so that it behaves like Claude does for me.
27. As a user, I want the risk documented that tools I've already allowed in my Claude settings (an email tool, say) run without asking, so that I know what I'm opting into.
28. As a user, I want the Hand-off to use my subscription the same way Lucy's own `claude` does, with the subscription guard applied, so that I'm never billed by accident.
49. As a user, I want to answer Claude's closing offer ("want me to apply the fix?") with "tell Claude yes, go ahead", even at a later Session, and have it carry on the same conversation, so that it keeps its context.
50. As a user, I want a Hand-off to see the environment my login shell sets up (tokens my MCP servers read), so that my integrations work under Lucy as they do in my terminal.
51. As a user, I want a permission request Claude makes after the Session ended raised at my next Session, so that the work isn't thrown away.

### Seeing and acting on the screen

29. As a user, I want to say "play the third video" after a search, and have Lucy look, find it and click it, so that I can carry on by voice.
30. As a user, I want her to follow links, press buttons, scroll and type into fields in any app, so that this isn't limited to one site.
31. As a user, I want her to look again after acting, to check it worked, so that she notices when a click missed.
32. As a user, I want her to look only when my request needs it, so that she isn't watching my screen otherwise.
33. As a user, I want her first use of the screen after lucyd starts to need my spoken yes ("I need to look at and use your screen. That lasts until Lucy restarts. Okay?"), and that yes to cover looking and acting until lucyd restarts, so that I know a screenshot is about to leave my machine without being asked again for every click or Session. (Amended 2026-11-18, ticket 55.)
34. As a user, I want a "looking" mark on the Indicator whenever she takes another look, so that I always know.
35. As a user, I want screenshots never logged, written anywhere but the Session transcript or sent over the public socket, and gone when that transcript is deleted at Session end (or at lucyd's next start after a crash), so that what's on my screen doesn't linger.
36. As a user, I want a setting that turns screen access off entirely, so that I can opt out.
37. (Withdrawn 2026-11-18, ticket 55: after the screen yes, no click needs another Confirmation.)
38. (Withdrawn 2026-11-18, ticket 55: there is no risky-word list.)
39. (Withdrawn 2026-11-18, ticket 55: clicks are no longer confirmed one by one.)
40. As a user, I want her never to type into a password field, so that secrets go through the Input dialog to the tool that needs them, never through the screen.
41. As a user, I want her to ask through the Input dialog, rather than guess, when the next step needs something typed, so that she doesn't fill in wrong details.
42. As a user, I want text on the screen treated as untrusted, so that a web page can't steer her.
43. As a Plugin author, I want the screen tools to live in the desktop Plugin, so that a GNOME or KDE Plugin can provide them its own way.
52. (Withdrawn 2026-11-18, ticket 55: keys are no longer checked.)
53. (Withdrawn 2026-11-18, ticket 55: there is no probe under a click.)
54. (Withdrawn 2026-11-18, ticket 55: there is no browser-accessibility offer.)

### Finding files

44. As a user, I want to say "show me my naval conference files in Documents" and have Lucy find them by name, so that I don't have to remember exact file names or paths.
45. As a user, I want my words matched in any order and any case, so that "naval conference" finds "1921 Naval Conference".
46. As a user, I want Lucy to open the file manager with what she found highlighted, so that I can see it at once.
47. As a user, I want to say "open the second one" and have it open in its default app, so that I can go straight to a file.
48. As a user, I want hidden and build folders left out of the search, and the roots and skip list configurable, so that results are relevant and fast.

## Implementation Decisions

### Hand-off (ADR 0007)

- **New Built-in tool:** `hand_to_claude {request, folder?}`, with no Confirmation. It starts a Hand-off, a new kind of Delegate beside Authoring. The fast tier passes the user's request in their words and the folder as they named it.
- **Folder resolution in lucyd:**
  - An absolute or `~` path is used as given.
  - A bare name is searched for under configured roots (`[handoff] roots`, default `~`, three levels deep), git repos first.
  - Several matches mean Lucy asks which one; none means she says so and nothing starts.
  - No folder means `~`.
- **The process:** the user's real `claude`, using their own settings sources and MCP config. It is not isolated and not under Landlock, and the subscription guard's environment scrub still applies. It runs on the user's default model, in the resolved folder.
  - **Environment:** at the start of each Hand-off lucyd runs `$SHELL -lic 'env -0'` and captures the result, so the user's login and interactive shell exports are there. It removes the billing variables from that environment itself and starts `claude` directly with it, so a shell startup file can't undo the guard. If the capture fails or takes over 5 s, lucyd falls back to its own environment and logs that (never the values). Lucy's relay is added as an extra MCP server for the Delegate tools and the permission tool. It runs in Claude's default permission mode with `--permission-prompt-tool` pointing at Lucy's permission tool.
- **Delegate tools for a Hand-off:**
  - `report_progress {text}`: notifications only, as in Authoring.
  - `ask_user {question}`: spoken when nobody is speaking.
  - `ask_typed {question, kind}`: opens the Input dialog; `kind` as for the Session's `ask_typed`, no `secret`.
  - The permission tool.
  - There is no `submit_plugin` and no `cannot`. The Hand-off ends when Claude's turn ends.
- **Permission tool:** suspends the call while Lucy speaks "Claude wants to {description}. Okay?". The description comes from Claude's request. The full tool name and input go to a notification. The answer goes through the same local yes-phrase match as every Confirmation. Anything else is a deny returned to Claude.
- **Results:** when Claude's turn ends, the fast tier speaks a one-to-three-sentence summary of the final message, and the full message goes to a notification. With no Session open, the notification goes out and the summary is offered at the next Session start, through the same keep-for-next-Session path Authoring uses for its questions and Approvals. Parked questions work the same way, and so do permission requests: one made with no Session open sends a notification at once and is asked at the next Session, while the budget clock keeps running.
- **Continuing:** `hand_to_claude` takes `continue` (bool). The fast tier sets it when the user is replying to Claude's last result. lucyd keeps the most recent finished Hand-off's conversation id and folder in its state dir, so it survives a restart, and resumes it with `--resume <id>` in that folder, with a fresh budget. With nothing to continue, Lucy says so and nothing starts. While a Hand-off runs, `continue` gets the queue offer like any second request.
- **Session while running:** Listening with the silence timer paused and a "working" mark on the Indicator. It is a separate mark from Authoring's "building" mark, and both can show.
- **Limits:**
  - One Hand-off at a time, separate from Authoring's queue. A second request offers to queue, with a queue of one.
  - Budget: `[handoff] budget_mins`, default 60, on the harness clock. At the budget the process is stopped and the notification carries the resume command.
  - "Cancel it" is matched locally. With both a Hand-off and an Authoring build running, Lucy asks which one.
- **Transcript:** kept. lucyd's transcript deletion covers only Lucy's own Sessions and Authoring Delegates, never a Hand-off's. The final notification (done, cancelled or over budget) carries `claude --resume <id>` and the folder.
- **Provider:** the Delegate kind is written so it doesn't depend on one Provider. Only `claude` ships; "tell Codex to …" is later work.
- **Public IPC:** the `state` event gains a "working" flag beside "building" (added within protocol 1). No request text, folder, tool input or result goes on the socket.

### Screen access (ADR 0008)

- **Desktop Plugin tools** (added to the Omarchy Plugin):
  - `screenshot_for_model {window?}`: the focused monitor or one window as an MCP image result, scaled down by the Plugin to the model's image size limit.
  - `click {x, y, button?}`
  - `type {text}`
  - `key {combo}`
  - `scroll {x, y, direction, amount}`
- **How they act:** coordinates are in screenshot pixels, and the Plugin maps them back to real pixels, so the core never deals with scaling. Input goes through Wayland's virtual-pointer and virtual-keyboard protocols, never `ydotool`.
- **The loop:** the Session model looks, acts and looks again through ordinary tool calls. There is no loop in Lucy's core, and no new Provider kind.
- **Core: image passthrough.** Proxied Plugin tool results may carry MCP image content through to the Thinking Provider. Images never go to logs, state files, notifications or the public socket. They exist on disk only inside the Session's `claude` transcript, which is deleted at Session end. At start, lucyd sweeps any of its own Session transcripts left behind by a crash.
- **Core: the screen yes (amended 2026-11-18, ticket 55).** The first screen tool call of any kind after lucyd starts needs a Confirmation ("I need to look at and use your screen. That lasts until Lucy restarts. Okay?"). A yes lets every screen tool run with no further Confirmation, in every Session, until lucyd restarts; it is held in lucyd's memory only, so a restart is the only way to take it back. A no, or no answer, returns `declined` and isn't remembered: the next screen tool call asks again. Any screen tool that runs shows a "looking" mark on the Indicator until its Turn ends (a `state` flag added within protocol 1). A screen tool whose Manifest sets `confirm = true` still asks its own Confirmation.
- **Core: the switch.** `[screen] enabled` (default true). When false, the screen tools are left out of the tool list.
- **Risky clicks and keys, the probe and the browser-accessibility offer (tickets 07, 10 and 14): removed 2026-11-18 (ticket 55).** After the screen yes nothing is left for them to gate. ADR 0008 records the tradeoff: a page carrying a prompt injection could get a send or a purchase through with no check.
- **Plugin: password fields (ticket 13).** The desktop Plugin refuses `type` when the focused element is a password field, by AT-SPI role. The refusal tells the model to use `ask_typed` or the Plugin's credential path. When the focused element can't be found, `type` goes ahead and the Plugin logs that the check couldn't run. The model never holds the user's secrets (they go through the Input dialog), so this stops a mistake, not a leak.
- **How AT-SPI finds the focus:** the Plugin follows AT-SPI focus events to know the focused element. Coverage is GTK3, Qt, Electron and Chromium-family browsers with accessibility on; GTK4 is unreliable; terminals, canvas and Xwayland apps return nothing.
- **How Plugins mark these tools:** one new hash-covered field per tool in the Manifest, `screen = "look" | "click" | "key" | "type" | "scroll"`, owned by ticket 06 (`type` and `scroll` added by 07, so `[screen] enabled = false` drops them too). Any marked tool triggers the screen yes and shows the "looking" mark when it runs.

### Finding files

- **New Built-in tools, with no Confirmation:**
  - `find_files {words, near?}` matches names only, containing every word, case-insensitive. `near` ranks matches inside a matching folder first. Folders rank before files, then newest first. At most 20 results, with a total count.
  - `show_files {paths}` highlights the items in the file manager over the `org.freedesktop.FileManager1.ShowItems` D-Bus call, or opens the parent folder with `xdg-open` if nothing answers.
  - `open_file {path}` opens a file with `xdg-open`, refusing executables, `.desktop` files and paths outside the roots.
- **The walk:** in lucyd, with the `ignore` crate, so there's no external dependency. `[files] roots` (default `~`) and `[files] skip` (default `node_modules`, `target`, `.cache`, `.git`, `venv`, `__pycache__`). Hidden folders are always skipped.
- **Privacy:** paths go to the Thinking Provider only, never to the bus, notifications or logs.
- Contents search is out of scope.

## Testing Decisions

- **What a good test is:** as in v0.1, it drives Lucy through her external surfaces (state events, notices, MCP traffic, spoken-text requests, files written) and never reaches into module internals.
- **Hand-off, in the lucyd harness:** the existing scripted Delegate (`thinking.delegate`) plays Claude. It calls `report_progress`, `ask_user`, `ask_typed` and the permission tool through its relay, and ends its turn. The tests cover:
  - Folder resolution: one match, several, none, an absolute path.
  - Permission yes, no and silence, with the notification body.
  - The summary spoken in the Session.
  - Surviving Session end and being offered at the next Session.
  - A parked question.
  - The budget on the harness clock.
  - "Cancel it" alone and with an Authoring build running.
  - The queue of one.
  - The "working" flag.
  - Nothing private on the bus.
  - A parked permission request, asked at the next Session.
  - `continue` resuming the last Hand-off's id and folder, including after a lucyd restart, and with nothing to continue.
- **Hand-off environment:** a fake `$SHELL` whose `env -0` output sets a token and `ANTHROPIC_API_KEY`: the token reaches `claude`, the key doesn't; a shell that hangs falls back after 5 s.
- **Hand-off against the fake `claude`:** with `Harness::with_thinking`, check that the real adapter starts `claude` with the user's settings sources, `--permission-prompt-tool`, the relay as an extra MCP server and the subscription-guard environment, and that it leaves the transcript in place. A live opt-in check against real `claude` is manual, because it spends the subscription.
- **Screen tools, in the Omarchy Plugin:** a new `Screen` trait inside the Plugin (capture, pointer, keyboard) with a fake in its MCP tests, since Wayland's virtual-input protocols can't be faked from outside. A manual check on real Hyprland covers the protocols.
- **Screen core, in the lucyd harness:** the fixture Plugin gains an image-returning tool and a click tool. The tests cover:
  - The image reaches the model, and never the logs, state dir or bus.
  - The screen yes: asked at the first screen tool call of any kind, carried into later Sessions, asked again after a restart, a no or no answer not remembered, and the "looking" flag.
  - After the yes, clicks, keys and typing run unasked; a `confirm = true` screen tool still asks.
  - `[screen] enabled = false`.
  - A leftover Session transcript swept at start.
- **Prior art:** `tests/authoring.rs` (scripted Delegate, parked questions, budget, cancel); `tests/confirmation.rs`; `tests/subscription_guard.rs` (real adapter against the fake `claude`); the news Plugin's MCP support harness.

## Out of Scope

- Hand-off to Codex ("tell Codex to …").
- Attaching to a Claude the user already has running (Herdr).
- An override list that forces a Confirmation on tools the user's Claude settings already allow.
- Driving the browser over the Chrome DevTools Protocol.
- AT-SPI targeting: clicking an element by its accessible identity rather than by screenshot pixels. AT-SPI is used in v0.2 only for the risk check and the password check.
- Turning on a browser's accessibility for the user.
- A separate vision Provider.
- Screen tools for GNOME, KDE or other desktops.
- Undoing a wrong click beyond looking again and reporting it.

## Further Notes

- ADR 0007 partly reopens ADR 0005's rejection of `--permission-prompt-tool`. ADR 0005 still holds for Lucy's own tools. The Hand-off case is argued there.
- AT-SPI findings behind ticket 10 (2026-11-17): on Wayland, screen-coordinate hit-testing doesn't work, so the Plugin converts to window coordinates; Chromium-family browsers expose an empty tree until accessibility is on (`org.a11y.Status IsEnabled` is false on this machine); Rust support is the `atspi` crate on zbus.
- Screenshots go to the Thinking Provider's vendor. The first-look Confirmation says so in plain words.
- Moved from v0.1 build tickets 38 and 41, which now point here.
