# 21: Plugin credentials

**What to build:** A Plugin that declares credentials by name gets them safely. lucyd stores the values in the Secret Service (`oo7`), and the user enters them in the TUI or a desktop prompt, never by voice and never through the model. They are passed to the Plugin as environment variables at spawn. No general OAuth flow.

Spec: Plugins (credentials).

**Blocked by:** 13, 20

**Status:** ready-for-agent

- [ ] Missing credentials are reported plainly, with where to enter them
- [ ] Credential values never appear in config, IPC, logs or model traffic
- [ ] A fixture Plugin receives its declared env vars and nothing more
