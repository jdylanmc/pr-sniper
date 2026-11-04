---
type: added
issue: 304
---

Path globs: a repository Paths gate decides which pull requests are queued, and secondary assignments review only revisions whose changed files match their own paths (skips shown as "Skipped: no matching files"); pull requests whose changed files cannot all be listed are parked for a human.
