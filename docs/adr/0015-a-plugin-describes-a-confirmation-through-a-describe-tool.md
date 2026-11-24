# A Plugin describes a Confirmation through a describe tool its Manifest names, and a mid-call question through `_meta`

A Confirmation's notification shows only the tool name and its raw arguments (`ids: ["1a1fdce3b98bc87a"]`), so the user says yes to something they can't see. Only the Plugin can turn those arguments into something readable, but Lucy's runtime asks for the yes before the call reaches the Plugin (ADR 0005).

A tool that needs a Confirmation may name a describe tool in its Manifest (`describe = "describe_trash_email"`). Before she speaks the Confirmation, Lucy calls that tool with the same arguments, under the same sandbox, and never lists it to the model. It returns `{"items": ["…", "…"]}` in `structuredContent`, one Markdown string per item, which Lucy shows in the Details window in place of the notification. If it fails, returns no items or takes longer than 5 s, she shows the raw notification and asks anyway. She does the same when the Plugin isn't running and still lacks a credential: a credential is asked for only after the yes, so she doesn't start the Plugin to describe.

A Plugin's mid-call question is already an elicitation from a running Plugin, so it attaches its items there, in the request's `_meta` under `lucy/items`, beside `lucy/secret` (ADR 0011).

Items go only to the Details window, never to the model or into logs.

## Considered Options

- The Plugin asks for its own yes in an elicitation carrying the items: moves the safety gate from Lucy's runtime into the Plugin, which ADR 0005 rejected.
- A Lucy-side tool the model calls with the details to show: the model would write what the user approves, so a misread or prompt-injected model could show one thing and do another.
- `_meta` items on the tool call itself: the runtime asks before the call reaches the Plugin, so there is nothing yet to read them from.
