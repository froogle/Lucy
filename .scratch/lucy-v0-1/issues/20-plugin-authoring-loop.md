# How does Lucy write a new Plugin when nothing can handle a request?

Type: grilling
Status: resolved
Blocked by: 19, 22

## Question

Walk the "find my Mount Rainier photo" scenario end to end: the fast tier recognises no Plugin fits, asks clarifying questions ("where are your photos?"), hands off to the advanced tier to research access and write the Plugin, builds and tests it, explains to the user what it does and can access, gets approval, guides any sign-in, then answers the original request. Decide what the advanced tier may do while building (network, filesystem, sandbox), where credentials live and who handles OAuth, how long the user waits and what Lucy says meanwhile, and what happens when building fails. Given [Can a personal app search a user's whole Google Photos library today?](22-google-photos-access.md), also confirm with the user where their photos actually live and settle the exact Mount Rainier success criterion (local folders by GPS/date, browser fallback, or a different first Plugin). Also decide how a delegated build reports progress or asks permission when no Session is open, consistent with "acts only when spoken to".

## Comments

- User input (2026-11-12): the user's photos are all in Google Photos. Principle the user set: when research finds a platform can't be accessed (API restrictions or terms, as with Google Photos), Lucy says so plainly ("I can't search your photos, but I can show them to you") and falls back to the best permitted option, e.g. opening the site in the browser so the user searches it themselves. This is an acceptable outcome for the Mount Rainier scenario.
- User input (2026-11-12): Lucy must be autonomous. The user shouldn't babysit how to connect; Lucy researches and works it out behind the scenes, then presents what she can do. Questions to the user are limited to things only they know or must do (which platform, signing in, approval).
- User input (2026-11-12): the final yes/no approval is always required, with no exception for read-only Plugins. Lucy must not "run wild": no working around access restrictions or site terms.
- From [Which tools does Lucy provide in v0.1, and which need a spoken "yes"?](09-tool-catalogue.md) (2026-11-12): the handoff to authoring is the fast tier calling the Built-in tool `request_plugin{goal, clarifications}`. Approval is heard by Lucy's runtime, never by the model (ADR 0005). The advanced tier's own tool set while building is still decided here.

- From [How does a Session move between listening, thinking and speaking, including interruptions?](10-session-lifecycle.md) (2026-11-12): the silence timeout runs only in Listening, and Thinking has no timeout. Decide here whether an authoring Delegate (minutes of work) keeps the Session open in Thinking, or ends it and reports back later. If it reports later, that touches the "Plugins running between Sessions" and "Proactive behaviour" fog.

- From [How is the code split into Rust crates, and how do lucyd and lucy talk to each other?](12-crate-architecture.md) (2026-11-12): every Thinking process, a Delegate included, reaches Lucy's MCP server through `lucy mcp-relay` with a per-spawn token. lucyd therefore knows which child is calling and can give the authoring Delegate its own tool set. Whether a Plugin SDK or template crate exists is also decided here.

## Answer

Grilled with the user on 2026-11-12/13. Glossary: added **Authoring**. No ADR: nothing here is hard to reverse.

- **Success criterion: two checks.**
  - **Honest limit:** with only Google Photos, Lucy researches, says plainly "I can't search your Google Photos, but I can open the search for you", and opens `photos.google.com/search/<query>` with `open_url`. No Plugin is written.
  - **Authoring:** the user makes a small Google Takeout export (one album or year containing the Rainier photo) into a local folder. Lucy writes a local photo-folder Plugin (EXIF GPS near Rainier, date, filename) and finds the photo.
- **The loop.** The fast tier clarifies what only the user knows ("where are your photos?"), then calls `request_plugin{goal, clarifications}`. lucyd spawns the advanced-tier Delegate with the goal and the original request stored alongside it.
- **The Session stays open.** Lucy says once "This is going to take a few minutes", then gets on with it.
  - The Session returns to Listening with the silence timer **paused** while a build runs (amends 10), so the user can ask other things meanwhile. The Indicator shows a "building" mark over the normal state.
  - **No spoken progress.** Milestones go to desktop notifications and the TUI. A `config.toml` setting turns the progress notifications on or off (default on).
  - Lucy speaks only when she needs the user: an `ask_user` question, a `cannot`, or the Approval summary. These wait until neither Lucy nor the user is speaking.
- **Build sandbox.** The Delegate (`claude`/`codex`) runs under Landlock:
  - read/write: the staging dir `~/.local/share/lucy/plugins/.build/<name>/`, toolchain caches (cargo registry), and the CLI's own config dir (needed for subscription login);
  - read-only: system and toolchain paths;
  - network: open (research, crates.io, API docs; Landlock can't filter hosts);
  - **no user data** before Approval. Tests use fixtures the Delegate creates itself.
  - It uses its native edit, Bash and web tools, all confined by the sandbox.
- **Delegate tools** on Lucy's MCP server (identified by the per-spawn token from 12). It gets no Plugin tools and no Built-in tools.
  - `report_progress{text}`: a notification (if enabled) plus the TUI, never spoken.
  - `ask_user{question}`: spoken in the open Session, or parked until the next one. Only for what only the user knows or must do (which platform, sign-in, a validation example), never how to connect.
  - `submit_plugin{}`: lucyd builds it, checks the Manifest against `tools/list`, and runs the Delegate's tests. If they pass, the Plugin goes to awaiting approval.
  - `cannot{reason, fallback}`: research found no permitted access. Lucy says so plainly and offers the fallback.
- **Approval and validation.**
  - If `submit_plugin` passes, Lucy speaks the full Approval summary from 19 straight away (Confirming).
  - On a yes, she immediately answers the stored original request and asks "Is that the one?" A no goes back to the Delegate as a fix through the 19 update flow (re-approval needed).
  - While building, the Delegate may ask for a validation example with `ask_user` when it judges one useful (for example "roughly when did you take it?"). This is its call at the time, not a required step.
- **Failure.**
  - Each build has a 20 min wall-clock budget (configurable). The staging dir is kept, so "try again" picks up where it stopped.
  - On a failed build, a timeout or tests that won't pass, the Delegate chooses between offering the best permitted fallback and asking a clarifying or validation question, then retrying within the remaining budget.
  - When it gives up, Lucy says plainly what failed.
  - One build runs at a time. A second request gets "I'm already building the photos Plugin, I'll do this after" and queues (a queue of one).
- **Session ends mid-build** (Indicator click, closing phrase): the build continues. Lucy says "I'll keep working on it and tell you when you're back." When it finishes she sends a notification, then offers the Approval once at the start of the next Session ("Your photo Plugin is ready. Want to hear about it?"). A parked `ask_user` is raised the same way. Only an explicit "cancel it" stops a build. The Delegate is a thinking process, not a Plugin, so v0.1's "no Plugins between Sessions" still holds.
- **Credentials** (the open point in 19).
  - Credentials declared by name are stored by lucyd in the Secret Service keyring (`oo7`) and handed to the Plugin as env vars at spawn.
  - The user enters them in the TUI or a desktop prompt, never by voice and never through the model.
  - Each Plugin gets a private writable state dir, `~/.local/state/lucy/plugins/<name>/` (Landlock-allowed automatically), for things like refreshed tokens.
  - **No general OAuth flow in v0.1.** It stays in the Email & calendar fog.
- **Plugin SDK (left open by 12): none in v0.1.**
  - Lucy ships a template folder, `/usr/share/lucy/plugin-template/`: a minimal `rmcp` stdio server, a `manifest.toml` skeleton and a fixtures test harness.
  - She also ships an authoring brief as the Delegate's system prompt: permitted access only, use `cannot` when access isn't permitted, fixtures only, Rust by default.
  - The news briefing is the worked reference. An SDK crate waits until a second Plugin shows what is shared.
