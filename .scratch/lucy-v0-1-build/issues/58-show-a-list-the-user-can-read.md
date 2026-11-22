# 58: Lucy can show the user a list of Markdown items any Plugin can use

**What to build:** When a Plugin tool needs a Confirmation, the only thing on screen is a notification, "Lucy needs your yes". Its body is the tool name and its raw arguments (`crates/lucyd/src/plugins/mod.rs`, the Confirmation's `Notification`). For example, when Lucy asked to trash an email, it showed:

```
gmail-manage__trash_email
ids: ["1a1fdce3b98bc87a"]
```

The user can't tell which email that is, so they're saying yes to something they can't see. Only the Plugin knows how to turn those IDs into something readable (sender, subject, date), and it has no way to show it. A Plugin could also put a mid-call question in the Input dialog, but that's a small box with one line of text, built for typing an answer, not for showing data.

A Hand-off has the same problem. Its "Claude needs your yes" notification (`crates/lucyd/src/hand_off.rs`) shows Claude's raw tool input.

Add a generic display to Lucy's core: a window that shows a list of items, each written in Markdown, for the user to read. Any Plugin can use it to show the data it needs the user to look at. Lucy's own tools can use it too, and so can a Hand-off's permission requests.

- Lucy's core owns the window, so every Plugin gets the same display and none draws its own.
- Each item is Markdown, rendered (headings, emphasis, lists, links, code), not shown as raw text.
- The list scrolls when it is longer than the window.
- When a Plugin shows data and asks a question about it, the user can read the data while they answer, whether the answer is a spoken Confirmation or typed in the Input dialog.

To define at triage:

- How a Plugin supplies the list for a Confirmation, which Lucy's runtime asks before the tool call reaches the Plugin (ADR 0005). Options include a second, read-only tool the Manifest names (such as `describe_trash_email`) that Lucy calls first, a Plugin-side elicitation that uses the list in `_meta` as ADR 0011 does for `secret`, or a tool on Lucy's side.
- Whether the raw notification stays as well, or the list replaces it.
- Whether the user can only read the list, or can also pick or tick items in it.
- When it closes: on the answer, when the user closes it, or when the Session ends.
- Where it draws: in the `lucy` client with ratatui, like the Input dialog, and how big.
- A `GLOSSARY.md` term for it, and whether it needs an ADR.

**Status:** moved to GitHub [#7](https://github.com/froogle/Lucy/issues/7)

Spec: none; raised by the user on 2026-11-21.

**Blocked by:** none
