---
type: fixed
issue: 339
---

A path-scoped secondary that already reviewed a revision keeps its report on the queue item when the PR state later moves on the same head, instead of showing as skipped with no matching files.
