# 57: A yes to a Hand-off's permission request lasts the whole Session

**What to build:** Today every permission request from a Hand-off's Claude asks its own Confirmation (v0.2 ticket 02, ADR 0007). The user was adding items to their Christmas shopping list by voice. For each item, Lucy asked for the list's URL, they gave it, and Claude edited the list. That worked well, but Lucy asked "Claude wants to … Okay?" before every single edit.

Change it so that:

- After the user says yes to a permission request, later requests for the same tool run with no Confirmation for the rest of the Session.
- A no, silence or an Indicator click is not remembered. The next request asks again.
- The yes is held in memory for the Session only and is never written to disk. The next Session asks again.
- The full tool name and input still go to a notification for every request, including the ones that don't ask.

Assumed, to confirm at triage:

- "The same tool" means the same tool name (for example, the editing tool for the list), not any permission request at all and not the exact same input.
- "Session" means Lucy's Session, not the Hand-off. A Hand-off that outlives its Session (v0.2 ticket 05) or is continued later (v0.2 ticket 12) asks again.

**Status:** needs-triage

Spec: none; raised by the user on 2026-11-21. Amend ADR 0007, the v0.2 spec's Hand-off section, and the `GLOSSARY.md` entry for Confirmation, which says most Confirmations cover one action.

**Blocked by:** none
