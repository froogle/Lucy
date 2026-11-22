# 54: A newly built Plugin's credentials are set up before its Approval

**What to build:** Today, when Authoring finishes a Plugin, Lucy asks for its Approval straight away (`Build::approve` in `crates/lucyd/src/authoring/mod.rs`). She asks for a missing credential only when a tool call needs it (ticket 44). In the 2026-11-18 Linear Plugin build, that made it hard to move between approving the Plugin, finding the API key and typing it in. The user had to find Linear's API key page alone, part way through a spoken flow.

When a finished Plugin declares credentials (`[access] credentials` in its Manifest), change the order to this:

1. Lucy opens the browser at the page where each credential is created, such as Linear's API key settings. She talks the user through what to do there: what to click, which scopes or permissions to choose and what to copy.
2. Lucy waits until the user says they have the credentials ready. Building takes a while, so the user may have moved on. Waiting doesn't block anything else.
3. Only then does she ask for the Plugin's Approval (the spoken Approval, unchanged; ADR 0005).
4. On a yes, she opens a `secret` Input dialog for each credential (the ticket 43/44 path) and stores them. Then she answers the original request with the Plugin, as she does today.

A Plugin with no credentials goes straight to Approval, as it does today.

Spec: none yet; raised by the user on 2026-11-18 after the Linear Plugin build. Amend the spec's Authoring and Plugins (credentials) sections.

**Blocked by:** none

**Seams:** extends Authoring's finish (`Build::approve`). Uses `Host::request_approval_when_quiet`, the `secret` Input dialog (ticket 43), `CredentialStore` (ticket 21) and `Launcher::open_url` as they are. May extend the Manifest (see Notes).

**Status:** moved to GitHub [#4](https://github.com/froogle/Lucy/issues/4)

**Notes:**

- Where the page and the steps come from is open. Options:
  - The Manifest declares a setup URL and short instructions for each credential. This is durable: `lucy credentials <plugin>`, hand-installed Plugins and ticket 44's prompt could all use them. But it changes the Manifest format and what Approval pins.
  - The Authoring delegate passes them to `submit_plugin`. This is simpler, but only covers freshly built Plugins.
- The URL comes from a Plugin that hasn't been approved yet, so treat it as untrusted. Open only `https` URLs, and show the URL in the notification. Opening a page asks for no Approval of its own, but say why that's acceptable in the spec amendment.
- "Ready" can come in a later Session. The Plugin's state (awaiting credentials, then awaiting Approval) needs to survive the Session ending, the way the kept "ready" notification and `When::Kept` questions do now. The scan report should tell the model so it can pick the flow back up.
- If the user says no at Approval, they've already been to the credential page but nothing has been stored. Nothing secret is typed before the yes.
- If the user cancels a credential dialog after a yes, the Plugin stays approved and ticket 44's call-time prompt still applies.

**Acceptance:**

- [ ] Harness test: a built Plugin with one credential opens its setup page, waits for the user's "ready", then asks for Approval, then opens the `secret` dialog, then answers the request
- [ ] A built Plugin with no credentials goes straight to Approval (existing test still passes)
- [ ] A declined Approval opens no credential dialog and stores nothing
- [ ] Saying "ready" in a later Session resumes at Approval
- [ ] Once, by hand: build a Plugin that needs an API key, and the page, talk-through, Approval and key entry run in that order

## Comments
