---
type: fixed
pr: 288
---

Query the Chocolatey community feed with an Atom/XML-only `Accept` header; the feed rejects any request that allows JSON with HTTP 406, which blocked `choco-publish` before push.
