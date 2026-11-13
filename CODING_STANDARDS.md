# Coding standards

Read during review. These are judgement calls; anything mechanical is a check in `scripts/check.sh`.

## Events reach every subscriber

A latest-value channel (`tokio::sync::watch`) holds snapshots only, such as what `status` returns. When the spec says something is emitted "for each change", deliver it through a queue so a slow or busy subscriber still receives every change in order.

## Tests prove ordering deterministically

A test asserting a sequence controls the ordering itself: it steps the fakes, awaits each effect, or drives a paused clock. A test that fails only sometimes on broken code proves nothing. Make its outcome independent of scheduling before relying on it.
