# A Hand-off runs the user's own Claude, unsandboxed, with its permission requests spoken as Confirmations

When the user says "tell Claude to …", Lucy starts a Hand-off: a Delegate that runs the user's real `claude`, with their own settings, MCP servers and permission rules, in the folder they named (or `~`). It is the first thing Lucy runs for a model outside a sandbox. Lucy's own `claude` (isolated config, no built-in tools, `dontAsk`) and the Authoring Delegate (Landlock, no user data) can't serve this request. The value of the request is the user's own integrations (Linear, email) and real work in their folders, and both of those processes are built to keep these out.

The gate is the user's own Claude permission settings plus Lucy's runtime. Whatever the user has already allowed in their Claude settings runs without asking. Any other permission request goes through `--permission-prompt-tool` to a Lucy MCP tool. Lucy turns it into a spoken Confirmation: "Claude wants to {description}. Okay?" Lucy hears the yes herself, and the full tool name and input go to a notification. Anything other than a clear yes is a deny sent back to Claude. The hand-off itself needs no Confirmation. The transcript is kept, since it is the user's own `claude` session, and the final notification carries the `claude --resume` command.

_Amended 2026-11-17._ lucyd runs as a systemd user service, so its environment lacks what the user's shell exports. A Hand-off captures the environment of the user's login, interactive shell (`$SHELL -lic 'env -0'`), removes the billing variables from it in lucyd, and starts `claude` directly. Starting `claude` inside the shell was rejected, because a shell startup file could re-export an API key after the subscription guard had removed it.

_Amended 2026-11-24 (#6)._ A yes to a permission request for one of the user's own integrations (an MCP tool, `mcp__<server>__<tool>`) covers that tool, by name, for the rest of the Session in which Lucy heard it, across Hand-offs. The question says so: "… Okay? I won't ask again for that this Session." A later request for that tool, including one already waiting to be asked, runs with no Confirmation, and its tool name and full input still go to a notification ("Claude is using a tool you allowed"). The yes is held in memory only and ends with the Session. A request made after the Session ends is held and asked at the next Session, like any other. A no, silence or an Indicator click isn't remembered. Claude's built-in tools (`Bash`, `Write`, `Edit`, `WebFetch`, …) still ask every time, because one name covers almost any action: a yes to `ls` must not let `rm` through. Asking for every integration request was rejected because it made a voice task like adding items to a list one by one tiresome. Matching built-in tools more narrowly (a Bash command prefix, a folder) is left until someone asks for it.

_Partly reopens ADR 0005_, which rejected `--permission-prompt-tool` as Lucy's safety gate because it ties the gate to one Provider. That still holds for Lucy's own tools. Here the actions are Claude's own tools and the user's own integrations, so only Claude can see them, and a Hand-off is Claude-only for now. The yes is still heard by Lucy's runtime, never by a model. The prompt text comes from Claude, not from a Manifest. So the notification with the full tool input is what the user can check.

The accepted risk: a rule the user already allowed in their own Claude settings (for example an email MCP tool) runs with no Confirmation. That is documented. An override list is left until someone asks for it.

## Considered Options

- A Plugin: Plugins are Landlock-sandboxed, stop at Session end and can't see the user's Claude config.
- Herdr (wayfinder 04): only attaches to an agent the user already has running, and scrapes replies off the terminal screen.
- `bypassPermissions`: no gate at all.
- A read-only or plan mode: safe, but can't do the work the user asked for.
- Confirming every hand-off before it starts: the user has just asked for it, and the real risks are the actions inside it, which already get a Confirmation each.
