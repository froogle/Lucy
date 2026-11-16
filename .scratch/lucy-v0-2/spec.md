# Lucy v0.2: Hand-off and the screen

Status: ready-for-agent

Synthesised from a grilling session on build tickets 34, 35, 38 and 41 (2026-11-16), the [v0.1 spec](../lucy-v0-1/spec.md) as amended that day, `GLOSSARY.md` and ADRs 0001–0008 (0007 and 0008 are new). Vocabulary follows the glossary, including the new terms **Hand-off** and **Input dialog**. Building starts once v0.1 acceptance (build ticket 24) passes. Tickets 01–05 are the Hand-off and 06–07 the screen; 04 also needs v0.1's Input dialog (build ticket 34), and 06 needs the Omarchy Plugin (build ticket 35).

## Problem Statement

Lucy refuses two kinds of request the user keeps making.

The first is "tell Claude to …". The user has Claude Code set up with their own folders, settings and integrations (Linear, email and others over MCP). They want to pass a job to it by voice: "tell Claude to fix the failing test in lucy", "tell Claude to grab my Linear tickets". Lucy says she can't. Her own `claude` is deliberately isolated: no user config, no built-in tools, only her own MCP server. Her Authoring Delegate is sandboxed away from the user's data. Raw shell access is out of scope.

The second is anything that needs her to see the screen. She can open YouTube and search, but "now play the third video" does nothing, because she can't see what's there or click it. That holds for every app, not just the browser.

## Solution

**Hand-off.** "Tell Claude to …" becomes a Hand-off: a new kind of Delegate that runs the user's own `claude`, with their own config, MCP servers and permission rules, in the folder they name (or their home folder). Anything their Claude settings already allow runs. Anything else, Claude asks for, and Lucy turns that into a spoken Confirmation she hears herself, with the details on screen. She keeps the Session open while it works, raises Claude's questions aloud (or in the Input dialog when the answer must be typed) and speaks a short summary at the end, with the full answer in a notification. If the Session has ended, the result waits for the next one. The transcript is kept so the user can resume it at a keyboard.

**Seeing and acting on the screen.** The desktop Plugin (the Omarchy Plugin first) gains screen tools: take a screenshot for the model, click, type, press keys, scroll. The Session model, which already reads images, looks, acts and looks again to check. The first look in a Session needs the user's spoken yes, because the image leaves the machine. Clicks on things that read as risky (send, buy, delete …) need a Confirmation, decided by a fixed word list, not by the model. She never types into a password field; secrets go through the Input dialog.

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

### Seeing and acting on the screen

29. As a user, I want to say "play the third video" after a search, and have Lucy look, find it and click it, so that I can carry on by voice.
30. As a user, I want her to follow links, press buttons, scroll and type into fields in any app, so that this isn't limited to one site.
31. As a user, I want her to look again after acting, to check it worked, so that she notices when a click missed.
32. As a user, I want her to look only when my request needs it, so that she isn't watching my screen otherwise.
33. As a user, I want the first look in each Session to need my spoken yes ("I need to look at your screen. Okay?"), so that I know a screenshot is about to leave my machine.
34. As a user, I want a "looking" mark on the Indicator whenever she takes another look, so that I always know.
35. As a user, I want screenshots never logged, saved to disk or sent over the public socket, and gone with the Session transcript, so that what's on my screen doesn't linger.
36. As a user, I want a setting that turns screen access off entirely, so that I can opt out.
37. As a user, I want a click on anything that reads as send, buy, pay, delete, submit, post or confirm to need my spoken yes, so that a misclick or a malicious page can't make her do something risky.
38. As a user, I want that risky-word list editable in config, so that I can add words that matter to me.
39. As a user, I want her to say what she's about to click ("the Play button on the third video") in the Confirmation, so that I know what I'm agreeing to.
40. As a user, I want her never to type into a password field, so that secrets go through the Input dialog to the tool that needs them, never through the screen.
41. As a user, I want her to ask through the Input dialog, rather than guess, when the next step needs something typed, so that she doesn't fill in wrong details.
42. As a user, I want text on the screen treated as untrusted, so that a web page can't steer her.
43. As a Plugin author, I want the screen tools to live in the desktop Plugin, so that a GNOME or KDE Plugin can provide them its own way.

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
- **The process:** the user's real `claude`, using their own settings sources and MCP config. It is not isolated and not under Landlock, and the subscription guard's environment scrub still applies. It runs on the user's default model, in the resolved folder. Lucy's relay is added as an extra MCP server for the Delegate tools and the permission tool. It runs in Claude's default permission mode with `--permission-prompt-tool` pointing at Lucy's permission tool.
- **Delegate tools for a Hand-off:**
  - `report_progress {text}`: notifications only, as in Authoring.
  - `ask_user {question}`: spoken when nobody is speaking.
  - `ask_typed {question, kind}`: opens the Input dialog; `kind` as for the Session's `ask_typed`, no `secret`.
  - The permission tool.
  - There is no `submit_plugin` and no `cannot`. The Hand-off ends when Claude's turn ends.
- **Permission tool:** suspends the call while Lucy speaks "Claude wants to {description}. Okay?". The description comes from Claude's request. The full tool name and input go to a notification. The answer goes through the same local yes-phrase match as every Confirmation. Anything else is a deny returned to Claude.
- **Results:** when Claude's turn ends, the fast tier speaks a one-to-three-sentence summary of the final message, and the full message goes to a notification. With no Session open, the notification goes out and the summary is offered at the next Session start, through the same keep-for-next-Session path Authoring uses for its questions and Approvals. Parked questions work the same way.
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
  - `screenshot_for_model {window?}`: the screen or one window as an MCP image result.
  - `click {x, y, describe, button?}`
  - `type {text}`
  - `key {combo}`
  - `scroll {x, y, direction, amount}`
- **How they act:** coordinates are in screenshot pixels. Input goes through Wayland's virtual-pointer and virtual-keyboard protocols, never `ydotool`.
- **The loop:** the Session model looks, acts and looks again through ordinary tool calls. There is no loop in Lucy's core, and no new Provider kind.
- **Core: image passthrough.** Proxied Plugin tool results may carry MCP image content through to the Thinking Provider. Images never go to logs, state files, notifications or the public socket.
- **Core: the first look.** The first screen tool call in a Session needs a Confirmation ("I need to look at your screen. Okay?"). A decline returns `declined` and holds for the rest of the Session. Later calls show a "looking" mark on the Indicator (a `state` flag added within protocol 1).
- **Core: the switch.** `[screen] enabled` (default true). When false, the screen tools are left out of the tool list.
- **Core: risky clicks.** Before forwarding `click`, lucyd matches `describe` against `[screen] risky_words` (default send, buy, pay, delete, submit, post, confirm), and against the AT-SPI text at the point once AT-SPI is added. A match needs a Confirmation spoken from `describe`. The match is local and never asked of the model.
- **Core: password fields.** `type` is refused when the focused element is a password field, by AT-SPI role where available. The refusal tells the model to use `ask_typed` or the Plugin's credential path instead.
- **How Plugins mark these tools:** screen tools are marked in the Manifest so the core knows which calls count as a look and which `click` gets the risk check. The Manifest shape for that mark is settled when ticket 06 is built: one new hash-covered field, owned by that ticket.
- AT-SPI for exact targets and the risk check is a later addition. v0.2 starts with screenshots only.

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
- **Hand-off against the fake `claude`:** with `Harness::with_thinking`, check that the real adapter starts `claude` with the user's settings sources, `--permission-prompt-tool`, the relay as an extra MCP server and the subscription-guard environment, and that it leaves the transcript in place. A live opt-in check against real `claude` is manual, because it spends the subscription.
- **Screen tools, in the Omarchy Plugin:** a new `Screen` trait inside the Plugin (capture, pointer, keyboard) with a fake in its MCP tests, since Wayland's virtual-input protocols can't be faked from outside. A manual check on real Hyprland covers the protocols.
- **Screen core, in the lucyd harness:** the fixture Plugin gains an image-returning tool and a click tool. The tests cover:
  - The image reaches the model, and never the logs, state dir or bus.
  - The first-look Confirmation, its decline holding for the Session, and the "looking" flag.
  - `describe` against the risk list, including a custom list.
  - `[screen] enabled = false`.
- **Prior art:** `tests/authoring.rs` (scripted Delegate, parked questions, budget, cancel); `tests/confirmation.rs`; `tests/subscription_guard.rs` (real adapter against the fake `claude`); the news Plugin's MCP support harness.

## Out of Scope

- Hand-off to Codex ("tell Codex to …").
- Attaching to a Claude the user already has running (Herdr).
- An override list that forces a Confirmation on tools the user's Claude settings already allow.
- Driving the browser over the Chrome DevTools Protocol.
- AT-SPI targeting (a later addition, see above).
- A separate vision Provider.
- Screen tools for GNOME, KDE or other desktops.
- Undoing a wrong click beyond looking again and reporting it.

## Further Notes

- ADR 0007 partly reopens ADR 0005's rejection of `--permission-prompt-tool`. ADR 0005 still holds for Lucy's own tools. The Hand-off case is argued there.
- Screenshots go to the Thinking Provider's vendor. The first-look Confirmation says so in plain words.
- Moved from v0.1 build tickets 38 and 41, which now point here.
