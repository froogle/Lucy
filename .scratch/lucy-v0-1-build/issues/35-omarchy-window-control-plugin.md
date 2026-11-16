# 35: A Plugin for controlling Omarchy

**What to build:** Lucy opens apps fine today, but can't do much else with the desktop. Lucy should ship with an Omarchy Plugin: an agent for the Omarchy machine, so a person can ask for anything they want done to their desktop and have it done. Examples: close windows, change the tiling mode, move windows to other workspaces, switch workspaces, make an app full screen, and so on. It is a Plugin rather than more Built-in tools, so other people can write the same kind of Plugin for other Linux desktops; the Omarchy one is the first and the pattern for the rest. Still to settle: how far "anything" goes (Hyprland dispatchers only, or also Omarchy's own commands such as themes, the bar, night light and screenshots); which actions need Confirmation; whether it's enabled only when Omarchy/Hyprland is detected; and whether `launch_app` stays a Built-in tool; and whether this Plugin provides the screen-capture and input primitives for ticket 41 (see and act on the screen). Shipped Plugins are already approved by lucyd when it scans them (spec, Plugins: Approval). Details to be settled in a grilling session.

**Status:** needs-triage

## Comments

2026-11-16: the user widened the scope. It should ship with Lucy, cover anything you'd want done to an Omarchy machine (not just windows), and be the model for Plugins for other Linux desktops.
