# Handing work to Claude

If you use Claude Code, Lucy can pass a job to it for you. Say:

- "Tell Claude to fix the failing test in my lucy folder."
- "Ask Claude to write up today's meeting notes in ~/notes."

This is a **Hand-off**. Lucy starts your own `claude`, with your own settings, MCP servers and permission rules, in the folder you named (or your home folder if you didn't name one), and gives it your request in your words. Claude then works on its own.

## While it works

- The job carries on after the Session ends. The Indicator shows a mark while it runs.
- Claude's progress appears in notifications.
- If Claude has a question for you, Lucy asks it out loud, next time you're talking to her.
- When it's finished, Lucy tells you the result: straight away if you're talking to her, or at the start of your next Session.
- To carry on where it left off, reply to its result: "Yes, go ahead and do that." Claude continues the same conversation, in the same folder.

Only one Hand-off runs at a time. Ask for a second and Lucy offers to queue it behind the first.

To stop one, say "cancel it". A Hand-off also stops after an hour; change this with `[handoff] budget_mins`.

## What Claude may do

A Hand-off is **not sandboxed**. It runs as your Claude normally does:

- **Anything you've already allowed** in your Claude settings (an `allow` rule, a tool you've told it to always run) runs without asking.
- **Anything else** that Claude would normally ask permission for, Lucy asks you instead, out loud:

  > "Claude wants to edit src/main.rs. Okay?"

  The full details of the request appear in a notification. Say "yes" to allow it. Anything else means no.
- If Claude needs you to type something, such as a long URL, Lucy opens her Input dialog for it. What you type there goes to Claude.

So before your first Hand-off, look at what your Claude settings already allow. A rule that lets Claude send email without asking, for example, will also let it send email during a Hand-off without asking.

## Billing

Lucy removes API-key variables (such as `ANTHROPIC_API_KEY`) from the environment before starting Claude, so a Hand-off runs on your Claude subscription and never bills an API key by accident.
