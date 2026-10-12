# Publish only revision-safe, owned review batches

## Context

PR heads move. Publishing a stale review, or touching someone else's pending review, harms real PRs.

## Decision

- One pending review per publication. Explicit commit SHA. Event `COMMENT`.
- Pre- and post-mutation rechecks of head and gates. Disabling Comment stops the attempt.
- **Withdraw publication confirmation** removes only the exact owned, verified pending batch. Never submits or deletes another pending review. Confirmed visible batch is irrevocable.
- Execution snapshots are immutable. Capture config at run, never rewrite. Legacy gaps shown as unavailable. Interrupted job keeps prior snapshot, revalidates current plan before retry.
- `state/publications.json` holds the ledger. Restart reconciles the full remote set. Uncertain create is never recreated.
- Publish Comment grant gates initial publication. Reply Comment is independent.
- Provider acceptance is not machine sign-off.
- New local review cannot hide a pending, uncertain, or published batch for the same revision + assignment.

## Consequences

- Stale head blocks publish. Needs a new revision (ADR 0008).
- History stable under later library edits.
- Foreign pending reviews untouched.
- Offline fixtures are not live acceptance. Live needs a disposable PR.
