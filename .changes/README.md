# Changesets

One file per PR: `.changes/<name>.md`. Only release automation edits
`CHANGELOG.md`; entries stay here until a release is sliced.

| Rule     | Value                                                                               |
| -------- | ----------------------------------------------------------------------------------- |
| Name     | Unique, lowercase `[a-z0-9._-]`, e.g. branch name or issue number                   |
| `type`   | `added`, `changed`, `deprecated`, `removed`, `fixed` or `security`                  |
| `issue`  | Optional; `291` or `291, 292`                                                       |
| `pr`     | Optional; PR number                                                                 |
| Body     | Curated user-facing text, [Keep a Changelog 1.1.0][kac]; no headings or blank lines |
| Required | Every PR, except docs-, CI- or test-only PRs labelled `no-changeset`                |
| Reserved | `README.md`, `config.json`                                                          |

```markdown
---
type: fixed
issue: 123
---

Keep queued reviews after a failed poll.
```

`type` also picks the next version: any `added` (a new feature) bumps Y
(`X.(Y+1).0`); every other type (fixes and hardening) bumps Z. Major X is
human-only: `config.json` `major`; no changeset or automation changes it.

Multiple outcomes: one file each. A body that starts with `- ` is used as-is;
otherwise it becomes one bullet. Missing `#issue`/`#pr` refs are appended.

| Command                                     | Does                                                                                    |
| ------------------------------------------- | --------------------------------------------------------------------------------------- |
| `node scripts/changesets.mjs validate`      | Check every `.changes/*.md`                                                             |
| `node scripts/changesets.mjs list`          | List pending entries                                                                    |
| `node scripts/changesets.mjs next`          | Print the next version derived from pending entries                                     |
| `node scripts/changesets.mjs release X.Y.Z` | Release automation only: fold into `## [X.Y.Z] - date`, delete, bump, rebuild MCP asset |

Automation and release flow: [`docs/releases.md`](../docs/releases.md#changesets).

[kac]: https://keepachangelog.com/en/1.1.0/
