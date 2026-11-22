# Coding standards

Read while writing code and during review. These are judgement calls; anything mechanical is a check in `scripts/check.sh`. Code written before these rules is brought in line when a ticket touches it.

Sources: [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/), [Microsoft's Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/), the [rustdoc book](https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html), the [Rust Style Guide](https://doc.rust-lang.org/style-guide/), [Clippy's lint groups](https://doc.rust-lang.org/clippy/lints.html) and [the Rust Book on errors](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html). Where they ask for more documentation than this file, this file wins.

## Code explains itself, not comments

A comment that restates the code, narrates its steps or labels a section is a defect. Say it in the code instead: rename, extract a function named for what the comment would have said, introduce a type, or name a constant. The comments allowed are:

- rustdoc on entry points (next rule);
- `// SAFETY:` before each `unsafe` block, naming the invariants it relies on;
- an assumption the code can't avoid, written as the `expect("...")` message, not as a comment.

No commented-out code and no TODOs; open work is a GitHub issue.

## Rustdoc documents entry points only

Write `//!` at each crate and binary root, and `///` on items another crate uses. Private and `pub(crate)` items, fields and tests get none: their names carry the meaning. The first sentence is a summary of at most 15 words and doesn't restate the signature. A public function that returns `Result` or can panic has `# Errors` or `# Panics`. Docs describe what the code does now, not how it came to be.

## Decisions go in ADRs

Why a design was chosen, what was rejected and how it got here go in `docs/adr/`, not in comments or rustdoc. A doc may link its ADR but doesn't repeat it.

## Names carry the meaning

Domain concepts use their `GLOSSARY.md` term. Names are short, at most about two words, and don't repeat their module (`wake::word`, not `wake::wake_word`). Conversions are `as_`, `to_` and `into_`; getters have no `get_`. Casing follows RFC 430.

## Types make wrong states unrepresentable

An argument that means something gets a type: an enum, not a `bool` or `Option` flag; a newtype for a unit or an ID. Check a value once, when it's built, behind a private field, so later code needn't check it again. Keep items private or `pub(crate)` unless another crate uses them.

## Errors are values, bugs panic

Something that can fail at runtime returns `Result`; a broken invariant panics. No `unwrap()` outside tests: when failure is impossible, `expect` says why. Don't throw away an error's source with `map_err(|_| ...)`.

## Lints are fixed, or expected with a reason

Fix a Clippy lint rather than silencing it. When one must stay, write `#[expect(clippy::..., reason = "...")]`, never `#[allow]`: a stale expectation then fails the build.

## Events reach every subscriber

A latest-value channel (`tokio::sync::watch`) holds snapshots only, such as what `status` returns. When the spec says something is emitted "for each change", deliver it through a queue so a slow or busy subscriber still receives every change in order.

## Tests check behaviour

A test drives the code through its public seams and checks what it does. A test that mirrors the implementation or restates its constants passes by construction and proves nothing. Prefer the harness or `tests/` when either would do; `unwrap` and `expect` are fine in tests.

## Tests prove ordering deterministically

A test asserting a sequence controls the ordering itself: it steps the fakes, awaits each effect, or drives a paused clock. A test that fails only sometimes on broken code proves nothing. Make its outcome independent of scheduling before relying on it.
