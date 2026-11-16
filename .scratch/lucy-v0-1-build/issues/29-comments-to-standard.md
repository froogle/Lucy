# 29: Bring comments and rustdoc in line with the coding standards

**What to build:** Every crate follows `CODING_STANDARDS.md`'s "Code explains itself, not comments" and "Rustdoc documents entry points only". Where a comment says something the code doesn't, say it in the code: rename, extract a function named for what the comment said, add a type or name a constant. Then delete the comment. Rationale that matters goes in an ADR under `docs/adr/`. Behaviour doesn't change.

Then make the plain-comment rule mechanical: `scripts/check-comments.sh`, run by `scripts/check.sh`, fails on any `//` comment in `crates/` that isn't rustdoc (`///`, `//!`) or `// SAFETY:`.

Size on 2026-11-15 (outside `#[cfg(test)]` modules, then `tests/`):

| Crate | `///` lines | plain `//` lines | comment lines in `tests/` |
|---|---|---|---|
| lucyd | 1,132 | 141 | 498 |
| lucy-ipc | 132 | 0 | 0 |
| lucy | 118 | 13 | 47 |
| lucy-config | 98 | 1 | 0 |
| lucy-news | 73 | 12 | 20 |
| lucy-indicator | 48 | 4 | 0 |

Spec: none; `CODING_STANDARDS.md`.

**Blocked by:** none. Don't run it alongside ticket 30: both touch most of `lucyd`.

**Seams:** none owned. Renames that reach another crate (anything in `lucy-ipc`'s schema, `lucy-config`'s config keys) are out of scope: those names are public contracts.

**Status:** resolved

**Notes:**

- Do it a crate at a time, smallest first, one commit each, so each diff is reviewable.
- Keep rustdoc on each crate and binary root (`//!`) and on items another crate uses. Check what another crate uses with the compiler, not by eye: make an item `pub(crate)` and see whether the workspace still builds.
- `lucy-ipc`'s rustdoc feeds the generated JSON Schema's descriptions; regenerate it (`LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`) and keep what third-party widget authors need.
- The harness helpers' rustdoc (`crates/lucyd/tests/harness/mod.rs`) is how tests find them; `docs/agents/working-in-lucy.md` already describes most of them. Move anything it doesn't cover there rather than losing it.
- Fixture files that need a header to explain what they stand in for (the fake `claude` and `codex`) aren't Rust; leave them.

**Acceptance:**

- [ ] No plain `//` comment in `crates/` except `// SAFETY:`
- [ ] Rustdoc only on crate and binary roots and on items another crate uses; first sentences at most 15 words; `# Errors` and `# Panics` where they apply
- [ ] Anything a deleted comment recorded that still matters is in the code, an ADR or `working-in-lucy.md`
- [ ] `scripts/check-comments.sh` exists, runs in `scripts/check.sh`, and fails on a stray `//` comment
- [ ] `scripts/check.sh` passes; no test changed except for its comments and names

## Comments

2026-11-16: built in slice 8 (`lucy-v0-1-slice-8`). Two ADRs added (0009, 0010); `scripts/check-comments.sh` also allows `// sleep-ok:`, which `check-test-sleeps.sh` needs.
