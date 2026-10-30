---
type: fixed
issue: 296
---

Autonomous release no longer fails `land` on the release PR's approval-pending `pull_request` runs; it waits only for the CI runs it dispatched.
