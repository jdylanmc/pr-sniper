---
type: changed
issue: 291
---

Record changelog entries as one `.changes/*.md` file per PR. A `main` workflow
consumes them into `CHANGELOG.md` `Unreleased`; a dispatched release cuts the
version, bumps every version field and tags after main CI passes.
