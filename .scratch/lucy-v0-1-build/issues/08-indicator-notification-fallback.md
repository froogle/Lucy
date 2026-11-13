# 08: Indicator and notification fallback

**What to build:** The user can see Lucy's state. `lucy-indicator` is a separate client that claims the `indicator` role. It draws a small clickable marker on the overlay layer at the bottom right (layer-shell plus tiny-skia, no keyboard interactivity, exclusive zone 0). It looks different for each state, uses Omarchy theme colours and reloads them when the theme changes. Clicking it sends `session.end`. Whenever no client holds the `indicator` role, lucyd shows desktop notifications instead (start, Confirming, end; the start and Confirming notifications have an End action, the end notification has none).

Spec: Processes and crates; Session state machine (Indicator table). ADR 0006.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Idle hidden; Listening slow pulse; Thinking spinner; Speaking level meter driven by `level`; Confirming warning pulse; error red flash
- [ ] Click ends the Session from any state
- [ ] Exits cleanly with a log line when layer-shell is unavailable
- [ ] Notification fallback whenever no client holds the `indicator` role, including after an Indicator crash (harness test); a third-party client can claim the role
- [ ] `lucy-indicator.service` user unit file
