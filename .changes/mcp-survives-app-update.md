---
type: fixed
issue: 400
---

Keep the Copilot CLI MCP server working after an in-place app update: it switches to the new bundle and refreshes its tools, reads tolerate newer fields, and changes against a mismatched app fail with `app_updated` instead of saving.
