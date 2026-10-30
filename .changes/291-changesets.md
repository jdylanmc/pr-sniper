---
type: changed
issue: 291
---

Record changelog entries as one `.changes/*.md` file per PR instead of editing
`CHANGELOG.md`. Slicing a release folds them into the version section, deletes
them, bumps every version field and tags after main CI passes.
