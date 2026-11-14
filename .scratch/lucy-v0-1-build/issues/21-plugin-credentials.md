# 21: Plugin credentials

**What to build:** A Plugin that declares credentials by name gets them safely. lucyd stores the values in the Secret Service (`oo7`), and the user enters them in the TUI or a desktop prompt, never by voice and never through the model. They are passed to the Plugin as environment variables at spawn. No general OAuth flow.

Spec: Plugins (credentials).

**Blocked by:** 13, 20

**Status:** resolved

- [x] Missing credentials are reported plainly, with where to enter them
- [x] Credential values never appear in config, IPC, logs or model traffic
- [x] A fixture Plugin receives its declared env vars and nothing more

## Comments

Resolved on `lucy-v0-1-slice-6` (3c50b29..c56f613). New adapter trait `lucy_config::credentials::CredentialStore` (real store `SecretService` on oo7; `Harness::credentials` fake). Values are read when a Plugin starts for a call and added to a cleared env (PATH, HOME, LANG, LUCY_PLUGIN_STATE_DIR plus the declared names); `tools/list` and Approval starts get none, and the brief and template say so. Credential names that are invalid or would clobber the base env (`PATH`, `HOME`, `LANG`, `LUCY_*`, `LD_*`) make the Manifest broken. A missing credential is reported before Confirmation, naming the key and `lucy credentials <plugin>`. `plugins.list` carries names only. Entry is the `lucy credentials <plugin>` page only; no desktop prompt was built. Not yet checked with a person: the real Secret Service (unlock, prompt, store, read) and the page in a real terminal.
