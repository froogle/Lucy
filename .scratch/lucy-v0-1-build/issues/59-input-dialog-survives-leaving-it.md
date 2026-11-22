# 59: The Input dialog stays open while the user goes to fetch what it asks for

**What to build:** When the user sets up a Plugin, Lucy puts up the Input dialog to ask for a credential such as an API key. Often the user doesn't have it yet. They need to go to a browser, sign in to the service, create the key, copy it and paste it into the dialog. As soon as they click anything other than the dialog, it goes away. So they can't get to the browser to make the key, and the Plugin can't be set up.

When the bar panel is running, the dialog is drawn in Lucy's panel (`omarchy-plugin/Panel.qml`), which opens to ask and closes when it loses focus. Otherwise lucyd opens `lucy input` in a terminal (`runner.rs`). The user reported this from the panel. Check the terminal path behaves the same way.

Change it so that:

- Clicking or switching elsewhere doesn't close the dialog or drop the question. The user can work in other windows and on other workspaces while it waits.
- The user can get back to it easily and paste into it: the dialog stays visible, or one click on Lucy's bar icon brings it back with the box focused and what they typed still there.
- It still ends the ways it does today when the user means it to: they close it, say a cancel phrase, or the Session ends.
- The timeout gives the user enough time to go and create a key. Today a dialog can time out (ADR 0011), and a few minutes may not be enough.

To find out first: whether clicking away just hides the panel while the question keeps waiting, or cancels it. If it only hides, the fix may only need a way back to it and a longer timeout.

**Status:** moved to GitHub [#8](https://github.com/froogle/Lucy/issues/8)

Spec: none; raised by the user on 2026-11-21.

**Blocked by:** none
