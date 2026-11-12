---
type: fixed
issue: 381
---

A transient provider error retries a repository poll on its own instead of parking it, never fails its pull requests, and sends one "Connection lost" notice per outage that opens the repository.
