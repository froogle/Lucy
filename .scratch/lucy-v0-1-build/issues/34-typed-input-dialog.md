# 34: A dialog for typed input

**What to build:** Some things can't be said out loud, such as passwords and API keys, and some are easier typed, such as long URLs or exact spellings. Lucy needs a way to ask for them: she pops open a dialog on screen, the user types the answer, and it comes back to her without being spoken or heard. Then she carries on with what she was doing. This belongs in Lucy's core, so Built-in tools, Plugins and Lucy herself can all ask for input the same way, mid-task. Secrets must not end up in transcripts or logs. Check how this fits with ticket 21 (Plugin credentials), whose missing-credential case currently points the user at `lucy credentials <plugin>` with no desktop prompt. Still to settle: what draws the dialog (Quickshell, like the bar panel in ticket 36, or a standalone window); what the Session does while it waits, and what happens on cancel or timeout; whether a secret answer goes back to the model or only to the tool or Plugin that asked; and which kinds of field it needs (plain text, secret, URL, choice). Details to be settled in a grilling session.

**Status:** needs-triage

## Comments

2026-11-16: the user wants this to be a core ability: anything in the system can open a dialog to ask for information (a URL, a password), then continue once it has the answer.
