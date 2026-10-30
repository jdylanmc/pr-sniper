---
type: changed
issue: 296
---

Releases run end to end in CI from one dispatch of the Changesets workflow:
it picks the version from pending changesets when none is given (`added` bumps
the minor version, anything else the patch), lands the release PR through
auto-merge, tags after exact-commit CI, and retries Homebrew tap publication on
notarization ticket lag. Re-dispatching resumes an unfinished release.
