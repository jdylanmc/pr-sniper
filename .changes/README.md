# Changesets

One file per PR: `.changes/<name>.md`. PRs never edit `CHANGELOG.md`.

| Rule     | Value                                                                               |
| -------- | ----------------------------------------------------------------------------------- |
| Name     | Unique, lowercase `[a-z0-9._-]`, e.g. branch name or issue number                   |
| `type`   | `added`, `changed`, `deprecated`, `removed`, `fixed` or `security`                  |
| `issue`  | Optional; `291` or `291, 292`                                                       |
| `pr`     | Optional; PR number                                                                 |
| Body     | Curated user-facing text, [Keep a Changelog 1.1.0][kac]; no headings or blank lines |
| Opt out  | Docs-, CI- or test-only PR: label `no-changeset`                                    |
| Reserved | `README.md`                                                                         |

```markdown
---
type: fixed
issue: 123
---

Keep queued reviews after a failed poll.
```

Multiple outcomes: one file each. A body that starts with `- ` is used as-is;
otherwise it becomes one bullet. Missing `#issue`/`#pr` refs are appended.

| Command                                     | Does                                                             |
| ------------------------------------------- | ---------------------------------------------------------------- |
| `node scripts/changesets.mjs validate`      | Check every `.changes/*.md`                                      |
| `node scripts/changesets.mjs consume`       | Append into `## Unreleased`, delete consumed files; idempotent   |
| `node scripts/changesets.mjs release X.Y.Z` | Consume, cut `## X.Y.Z - date`, bump versions, rebuild MCP asset |

Automation and release flow: [`docs/releases.md`](../docs/releases.md#changesets).

[kac]: https://keepachangelog.com/en/1.1.0/
