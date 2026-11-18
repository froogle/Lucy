# 06: Look at the screen

**What to build:** Lucy can look at the screen when a request needs it ("what's on my screen?"). The Omarchy Plugin gains `screenshot_for_model {window?}`, which returns an MCP image of the focused monitor (or one window), scaled down by the Plugin to the model's image size limit. Lucy's core passes images from Plugin tools through to the model, and never to logs, state files, notifications or the public socket. They exist on disk only in the Session's `claude` transcript, deleted at Session end; at start, lucyd sweeps any of its Session transcripts a crash left behind. The first look in each Session needs a spoken Confirmation ("I need to look at your screen. Okay?"); a decline holds for the Session. Later looks show a "looking" mark on the Indicator. `[screen] enabled = false` leaves screen tools out.

Spec: [v0.2](../spec.md), Screen access. ADR 0008. Stories 32–36, 42, 43.

**Blocked by:** lucy-v0-1-build 24 (v0.1 acceptance), lucy-v0-1-build 35 (Omarchy Plugin)

**Seams:** owns the Manifest mark for screen tools (one hash-covered field per tool, `screen = "look" | "click" | "key" | "probe"`; 07 and 10 use the last three), the start-up transcript sweep, image passthrough in proxied results, the `[screen]` config section, the "looking" `state` flag and the `Screen` trait inside the Omarchy Plugin (capture now; pointer and keyboard in 07).

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.
- No new Provider kind: the Thinking Provider reads the image.

**Acceptance:**

- [ ] The Omarchy Plugin's `screenshot_for_model` is tested through its MCP interface with a fake `Screen`
- [ ] In the harness, a fixture Plugin's image result reaches the model, and nothing of it reaches logs, the state dir, notifications or the bus
- [ ] The first look asks; a decline holds for the Session; later looks set the "looking" flag (schema regenerated)
- [ ] `[screen] enabled = false` drops the marked tools from the tool list
- [ ] A Session transcript left behind (simulated crash) is swept when lucyd starts
- [ ] Once, by hand on Omarchy: "what's on my screen?" gets a sensible answer after a yes

## Comments

2026-11-17: built in slice 1 (`lucy-v0-2-slice-1`). The Plugin captures with `grim`. Images are scaled to at most 1568 px on the long edge and 1.15 MP (the smallest Claude limit, so the API never rescales and 07's coordinates hold). The "looking" mark shows on every allowed look, the first included, and lasts until the Turn ends: cleared per call, it flashed too briefly to see. Any answer but yes, a cut-in included, declines for the Session. lucyd holds rmcp's log to warnings, since at trace it logged whole tool results, images included (ADR 0008). Hand check left: on Omarchy, "what's on my screen?" after a yes.

2026-11-18: capture with `grim` never worked inside the Plugin sandbox (it needs `/dev/shm`). [15](15-capture-the-screen-without-grim.md) replaces it with the Plugin's own capture over Wayland.
