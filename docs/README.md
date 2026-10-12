# Docs

Index of everything under `docs/`. Start with the [main README](../README.md); agent rules live in [AGENTS.md](../AGENTS.md), domain terms in [CONTEXT.md](../CONTEXT.md).

## Reference

| Doc                                        | Purpose                                                                          |
| ------------------------------------------ | -------------------------------------------------------------------------------- |
| [panel-and-tray.md](panel-and-tray.md)     | Tray menu, retained panel, navigation state, Queue/Running/Reviewed tabs         |
| [settings.md](settings.md)                 | Settings UI: accounts, repos, assignments, Agents, Doctrines, Preferences, Genie |
| [review-queue.md](review-queue.md)         | Queue states, retry/park, local Agent reviews, notifications (macOS)             |
| [polling.md](polling.md)                   | Schedule, admission filters, lifecycle reads, GitHub connection and PR reads     |
| [provider-writes.md](provider-writes.md)   | Publication, thread/mention replies, Approve and Merge mechanics                 |
| [active-results.md](active-results.md)     | `result_page` / `result_detail` API, summary index, terminal cleanup             |
| [local-data.md](local-data.md)             | Data root, state files, atomic replace, recovery, retention journal              |
| [diagnostics.md](diagnostics.md)           | Retained logs, attempt records, caps, redaction                                  |
| [copilot-settings.md](copilot-settings.md) | Copilot accounts, returned model catalogs, per-Agent models                      |

## Platform and release

| Doc                                                                | Purpose                                                             |
| ------------------------------------------------------------------ | ------------------------------------------------------------------- |
| [macos-development.md](macos-development.md)                       | macOS requirements, dev run, signing, bundle, install, verification |
| [releases.md](releases.md)                                         | macOS release pipeline, notarization, Homebrew cask                 |
| [windows-development.md](windows-development.md)                   | Windows build, tests, CI artifact, native acceptance                |
| [windows-packaging.md](windows-packaging.md)                       | Unsigned NSIS installer and Chocolatey candidate lane               |
| [windows-notifications.md](windows-notifications.md)               | Windows toast identity and activation                               |
| [../tests/macos-acceptance.md](../tests/macos-acceptance.md)       | Native macOS window acceptance                                      |
| [../tests/macos-notifications.md](../tests/macos-notifications.md) | Native macOS notification acceptance                                |

## Decisions ([adr/](adr/))

| ADR                                                                 | Decision                                                |
| ------------------------------------------------------------------- | ------------------------------------------------------- |
| [0001](adr/0001-macos-foundation.md)                                | Keep the macOS tray host native and its surfaces local  |
| [0002](adr/0002-bind-repositories-to-provider-accounts.md)          | Bind every repository to an explicit provider account   |
| [0003](adr/0003-github-browser-oauth.md)                            | Use GitHub device authorization for OAuth accounts      |
| [0004](adr/0004-windows-persistence-foundations.md)                 | Keep Windows persistence private                        |
| [0005](adr/0005-windows-accounts-runtime.md)                        | Reuse account records behind native Windows credentials |
| [0006](adr/0006-windows-tray-startup.md)                            | Reuse the tray host; Windows startup opt-in             |
| [0007](adr/0007-retry-then-park-single-inbox.md)                    | Retry transient failures, then park in one inbox        |
| [0008](adr/0008-one-review-per-head-sha.md)                         | One review per head SHA                                 |
| [0009](adr/0009-admission-filters-first-match-wins.md)              | Ordered admission filters; first enabled match wins     |
| [0010](adr/0010-native-mutation-owner-intent-before-effect.md)      | Native host owns mutations; intent before effect        |
| [0011](adr/0011-separate-github-and-copilot-identities.md)          | Separate GitHub and Copilot identities                  |
| [0012](adr/0012-resource-scoped-saves-and-save-as-authorization.md) | Save per resource; Save is the authorization            |
| [0013](adr/0013-shared-ai-capacity.md)                              | One shared AI capacity pool; serial provider writes     |
| [0014](adr/0014-single-global-cron-schedule.md)                     | One global cron schedule                                |
| [0015](adr/0015-local-only-results-and-retention-journal.md)        | Local-only results; retention journal                   |
| [0016](adr/0016-revision-safe-publication.md)                       | Revision-safe, owned review batches                     |
| [0017](adr/0017-opt-in-provider-actions.md)                         | Opt-in, primary-only, fail-closed provider actions      |
| [0018](adr/0018-retained-hidden-panel.md)                           | Retained hidden tray panel                              |
| [0019](adr/0019-bounded-content-free-diagnostics.md)                | Bounded, content-free diagnostics                       |
| [0020](adr/0020-constrained-host-owned-review-tools.md)             | Three host-owned review tools, not a sandbox            |

## Specs and design

- [agent/specs/pr-sniper-mvp.nano.md](agent/specs/pr-sniper-mvp.nano.md): approved MVP spec (also `.full.md`).
- [agent/specs/pr-sniper-vnext.nano.md](agent/specs/pr-sniper-vnext.nano.md): vNext spec (also `.full.md`).
- [agent/specs/pr-sniper-vm-regression.nano.md](agent/specs/pr-sniper-vm-regression.nano.md): VM regression spec (also `.full.md`).
- [agent/discovery/](agent/discovery/): discovery notes for each spec.
- [agent/design/settings-default.md](agent/design/settings-default.md): Settings design record; approved mockups in [agent/design/settings-approved-a/](agent/design/settings-approved-a/).
- [../prototypes/v2/README.md](../prototypes/v2/README.md): vNext design prototype. Open `prototypes/v2/index.html`; design evidence, not product behavior.

## Agent workflow docs

- [agents/domain.md](agents/domain.md): how domain docs are consumed.
- [agents/issue-tracker.md](agents/issue-tracker.md): GitHub issue tracker conventions.
- [agents/triage-labels.md](agents/triage-labels.md): triage labels.
- [agents/commit-style.md](agents/commit-style.md): commit messages.
- `screenshots/`: README and docs images.
