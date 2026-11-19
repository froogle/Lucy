# Plugins

A **Plugin** is an ability Lucy can add: reading the news, managing your windows, sorting your photos. Each one is a small program with a **Manifest**, a short description of what it handles and what it needs access to.

Lucy ships with two Plugins:

- **news**: reads the official feeds of the news Sources you choose.
- **omarchy**: windows, workspaces, layouts, themes, the bar, night light, screenshots, and seeing and using your screen. It only switches on when you're running Omarchy.

Beyond these, Plugins come from two places: Lucy writes them herself when you ask for something she can't do yet, or a person writes one and you install it.

## Approval

Lucy never uses a Plugin you haven't approved. When there's a new one, or one has changed, she tells you at the start of your next Session and offers to go through it. If you'd like to, she reads it out, for example:

> "The photos Plugin needs your Approval. It handles sorting your photos into folders by date. It can read ~/Pictures and write to ~/Pictures. It never asks you before acting. Do you approve it?"

She tells you what it handles, what it can reach (files, network, other programs, credentials), and which actions it will ask you about first. Say "yes" to approve it. Anything else means no.

Some things to know about Approvals:

- **Every Plugin needs one**, whether Lucy or a person wrote it. Only the two shipped Plugins don't, since they're installed by root with Lucy herself.
- **Any change needs a fresh one.** An Approval is tied to the exact Manifest and program you approved. If either changes, by anyone, Lucy reads you what changed and asks again.
- **Approvals are only given by voice.** The `lucy` menu can revoke an Approval, but can't grant one. That way you always hear what a Plugin can do before it runs.
- **A Plugin can't approve itself.** Approvals are kept in Lucy's own state folder (`~/.local/state/lucy/approvals.toml`), out of every Plugin's reach.

## Asking Lucy to build a Plugin

When you ask for something none of her tools can do, Lucy can write a Plugin for it. She asks only what she needs to know from you (where your files are, which service you use), says it will take a few minutes, and starts building in the background. You can carry on talking or end the Session.

- Her advanced Model tier writes the Plugin, in a sandbox where it can't see any of your files.
- It writes tests for the Plugin and runs them before it's done.
- When it's ready, Lucy tells you and asks for your Approval. Once you approve it, she answers your original request with it.
- A build gives up after 20 minutes. Say "cancel it" to stop it sooner.

If a Plugin she wrote stops working, ask her to fix it; that's another build, and another Approval.

## Installing a Plugin someone else wrote

Copy the Plugin's folder into `~/.local/share/lucy/plugins/`. The folder's name must match the `name` in its `manifest.toml`:

```
~/.local/share/lucy/plugins/
└── weather/
    ├── manifest.toml
    └── weather          ← the program the Manifest's `command` names
```

Next time you talk to Lucy, she'll mention it's waiting for your Approval.

Only install Plugins from people you trust. The sandbox and the Approval help, but a Plugin is still a program running on your computer.

A Plugin you install with the same name as a shipped one takes its place.

## Managing Plugins

By voice:

- "Turn off the news Plugin." / "Turn it back on."
- "Roll back the photos Plugin." Lucy keeps the previous approved version of each Plugin.
- "Remove the photos Plugin." She asks you to confirm first. Shipped Plugins can be turned off, but not removed.

Or with `lucy`, in the **Plugins** and **Approvals** sections.

If a Plugin is broken (its Manifest doesn't parse, or it won't start), Lucy leaves it out and tells you once, at the start of a Session.

## Credentials

Some Plugins need a password or an API key, which their Manifest lists as **credentials**. Lucy keeps these in your system keyring (GNOME Keyring, KeePassXC or anything else that speaks the Secret Service API), never in a file.

Enter them ahead of time:

```sh
lucy credentials <plugin-name>
```

Or let Lucy ask: the first time a Plugin needs a credential you haven't entered, she opens an Input dialog for it. Either way, what you type goes to your keyring and to that Plugin only. The thinking model never sees it.

## The sandbox

Each Plugin runs in its own sandbox, using Linux's Landlock. It can read the system's programs and libraries and its own folder, and write only its own private state folder. Anything more (your Pictures folder, another program) has to be in its Manifest, and Lucy reads it out when asking for your Approval.

The network is the exception for now: a Plugin's network hosts are declared and read out to you, but not yet enforced.
