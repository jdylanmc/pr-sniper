---
type: added
issue: 365
---

A secondary assignment's Paths now scope its review as well as trigger it: it reads, searches and comments only on matching changed files (`!**/*.md` reviews everything except Markdown). Each Paths row has an include/exclude toggle, and the summary shows the effective list (`All −1` for exclude-only).
