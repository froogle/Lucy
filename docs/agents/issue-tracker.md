# Issue tracker: Local Markdown

Issues and specs for this repo live as markdown files in `.scratch/`.

## Conventions

- One feature per directory: `.scratch/<feature-slug>/`
- The spec is `.scratch/<feature-slug>/spec.md`
- Implementation issues are one file per ticket at `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01`, never a single combined tickets file
- Triage state is recorded as a `**Status:** <role>` line in each issue file, below **What to build** (see `triage-labels.md` for the role strings). Wayfinder tickets write plain `Status:`. `scripts/tickets.sh [issues-dir]` lists every ticket's status and blockers, for finding the frontier
- Comments and conversation history append to the bottom of the file under a `## Comments` heading

## Slices

A **slice** is the build tickets that are ready to start when the slice begins; tickets they unblock wait for the next slice. Slice N is built on `lucy-v0-1-slice-N`, branched from `main`. Once its review fixes are merged and its tickets resolved, the slice is merged into `main` locally (no push).

## When a skill says "publish to the issue tracker"

Create a new file under `.scratch/<feature-slug>/` (creating the directory if needed).

## When a skill says "fetch the relevant ticket"

Read the file at the referenced path. The user will normally pass the path or the issue number directly.

## Wayfinding operations

Used by `/wayfinder`. The **map** is a file with one **child** file per ticket.

- **Map**: `.scratch/<effort>/map.md` (the Notes / Decisions-so-far / Fog body).
- **Child ticket**: `.scratch/<effort>/issues/NN-<slug>.md`, numbered from `01`, with the question in the body. A `Type:` line records the ticket type (`research`/`prototype`/`grilling`/`task`); a `Status:` line records `claimed`/`resolved`.
- **Blocking**: a `Blocked by: NN, NN` line near the top. A ticket is unblocked when every file it lists is `resolved`.
- **Seams**: a `Seams:` line under `Blocked by:` names the shared seams the ticket touches (adapter traits, lucyd harness helpers, public IPC types, config sections, MCP tool-server hooks, Session states), as `owns X (also used by NN)` or `uses Y (owned by NN)`. Exactly one open ticket owns each new shared seam. An implementer builds only the seams its ticket owns, and uses or extends the ones it doesn't. A ticket that needs a shared seam no ticket owns says so in its report rather than inventing one.
- **Frontier**: scan `.scratch/<effort>/issues/` for files that are open, unblocked, and unclaimed; first by number wins.
- **Claim**: set `Status: claimed` and save before any work.
- **Resolve**: append the answer under an `## Answer` heading, set `Status: resolved`, then append a context pointer (gist + link) to the map's Decisions-so-far in `map.md`.
