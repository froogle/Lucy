# Issue tracker: GitHub Issues

Issues and specs for this repo live in GitHub Issues on [froogle/Lucy](https://github.com/froogle/Lucy/issues). Use the `gh` CLI for every operation.

`.scratch/` is the archive of the tickets, specs, research and prototypes from before the move (2026-11-22). ADRs and docs still link to it. Don't add tickets there; open work that was still open there has moved to GitHub, and each moved file's `Status:` line names its issue.

## Conventions

- One issue per ticket. Its title is the ticket's name, without a number; GitHub numbers it.
- The body keeps the build-ticket layout: **What to build**, Spec, **Blocked by**, **Seams**, **Notes**, **Acceptance**. There is no `Status:` line: triage state is the issue's label (see `triage-labels.md` for the strings), and done is the issue being closed.
- A spec is an issue labelled `spec`; its tickets link to it on their `Spec:` line.
- **Blocking**: a `**Blocked by:** #N, #N` line. A ticket is unblocked when every issue it lists is closed.
- Comments and conversation history are issue comments.
- A commit or PR that finishes a ticket says `Closes #N`.
- `gh issue list --state open` lists the open tickets, with their labels; that is how to find the frontier.

## Slices

A **slice** is the build tickets that are ready to start when the slice begins; tickets they unblock wait for the next slice. A slice is built on its own branch, branched from `main`. Once its review fixes are merged and its tickets closed, the slice is merged into `main` and pushed. After any ticket is done and merged, run `scripts/clean.sh` (see `working-in-lucy.md`).

## When a skill says "publish to the issue tracker"

Create a GitHub issue: `gh issue create -t "<title>" -l <label> -F <body-file>`.

## When a skill says "fetch the relevant ticket"

`gh issue view <N> --comments`. The user will normally pass the issue number directly.

## Wayfinding operations

Used by `/wayfinder`. The **map** is one issue with one **child** issue per ticket.

- **Map**: an issue labelled `map`, holding the Notes / Decisions-so-far / Fog body. Edit it with `gh issue edit <N> -F <body-file>`.
- **Child ticket**: an issue whose body starts with `Map: #<map>` and a `Type:` line (`research`/`prototype`/`grilling`/`task`), with the question in the body.
- **Blocking**: a `Blocked by: #N, #N` line near the top. A ticket is unblocked when every issue it lists is closed.
- **Seams**: a `Seams:` line under `Blocked by:` names the shared seams the ticket touches (adapter traits, lucyd harness helpers, public IPC types, config sections, MCP tool-server hooks, Session states), as `owns X (also used by #N)` or `uses Y (owned by #N)`. Exactly one open ticket owns each new shared seam. An implementer builds only the seams its ticket owns, and uses or extends the ones it doesn't. A ticket that needs a shared seam no ticket owns says so in its report rather than inventing one.
- **Frontier**: the map's open child issues that are unblocked and unassigned; lowest number wins.
- **Claim**: `gh issue edit <N> --add-assignee @me` before any work.
- **Resolve**: comment the answer under an `## Answer` heading, close the issue, then add a context pointer (gist + link) to the map's Decisions-so-far.
