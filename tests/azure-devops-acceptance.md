# Azure DevOps live acceptance

Reusable live procedure for the Azure DevOps provider: polling, admission,
path gates, explicit intake, publication and own-thread resolution. Automated
tests use fakes; they do not prove live provider behavior. Approve is a
reviewer vote and is out of scope here; merge is never offered.

## Safety and evidence

- Use a disposable Azure DevOps repository you own. Placeholders: `<org>`,
  `<project>`, `<repo>`. Never name real organizations, projects or people in
  recorded evidence that leaves your machine.
- Run an isolated app profile (absolute `PR_SNIPER_DATA_DIR`, unique
  `PR_SNIPER_KEYCHAIN_SERVICE`; see `AGENTS.md`). Azure DevOps auth is the
  Azure CLI token source ([ADR-0026](../docs/adr/0026-azure-devops-auth-via-azure-cli.md));
  `az login` before launch.
- Approval stays **off** on every assignment. Never complete, auto-complete or
  bypass policies on test PRs.
- Record candidate commit, app version, Azure CLI version, PR IDs, and per check
  the MCP results (`list_queue`, `inspect`, `read_evidence`) and the PR threads
  (`GET …/pullRequests/{n}/threads`). A missing observation is unverified, not
  passed.

## Test doctrine

Save one deterministic doctrine (`save_doctrine`) and use it on both
assignments:

- Flag only lines containing `PROBE-DEFECT-<X>` (`<X>` = one letter), one
  inline finding per marker line, on that line. Flag nothing else: no style,
  typo or summary comments.
- Before publishing, read existing threads. Reuse your own open thread for a
  marker; never open a second one.
- A marker that is gone from the head = fixed: resolve your own thread for it.
- No markers and no own open threads: sign off with no comment.

## Fixtures

Target branch `main` holds `probe/README.md` (any text, no markers). Each
check uses its own topic branch and PR unless stated.

| File                | Content                                        |
| ------------------- | ---------------------------------------------- |
| `probe/defect.md`   | Prose with a comment typo and `PROBE-DEFECT-A` |
| `probe/data.txt`    | Any text, no markers                           |
| `probe/notes.md`    | Any text, no markers                           |
| `outside/defect.md` | `PROBE-DEFECT-Z`                               |

## Setup over MCP

1. `list_repository_accounts` (`kind: repository`): Azure DevOps setup state
   ready; bound identity connected in `<org>`. The PR author for the probes is
   a second identity or the bound one; record which.
2. `resolve_repository` with `https://dev.azure.com/<org>/<project>/_git/<repo>`:
   `provider: azure_devops`, `provider_repository_id`
   `{organization}/{project ID}/{repository ID}`.
3. `save_doctrine` (test doctrine), then `save_agent` twice: **primary** and
   **secondary** (Copilot account + model, doctrine attached).
   `validate_agent` both.
4. One `save_repository` add, `expected: null`, **`enabled: false`**, carrying
   the full configuration (a disabled add may carry configuration,
   [#315](https://github.com/jdylanmc/pr-sniper/pull/315); an enabled add is
   refused):
   - Admission **Filtered**: one include-users rule naming the PR author
     (identity GUID + display name).
   - Repository `paths`: `probe/**`, `!probe/README.md`.
   - Primary assignment: no `paths`; Publish Comment + Reply Comment; Approval
     off.
   - Secondary assignment: `paths` `**/*.md`; Publish Comment.
   - Grants need `allow_grants`, else `explicit_grant_required`.
   - Use `resources.result.values` for exact field shapes; `validate_repository`
     first.
5. A second `save_repository` with the returned revision sets
   `enabled: true`.

## Checks

| #   | Action                                                                                                                              | Required observation                                                                                                                                                                                  |
| --- | ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | PR 1 adds `probe/defect.md`. Wait for the scheduled poll.                                                                           | Queued and reviewed by primary and secondary. One active inline thread per assignment on the `PROBE-DEFECT-A` line (`threadContext.filePath` + right-side line), marker footer present. Nothing else. |
| 2   | Minor revision: push a fix for the comment typo only.                                                                               | New revision reviewed. No new thread or comment; check 1 threads unchanged.                                                                                                                           |
| 3   | Major revision: add a line with `PROBE-DEFECT-B`.                                                                                   | Exactly one new thread per assignment for `B`. No second thread for `A`.                                                                                                                              |
| 4   | Remove both markers.                                                                                                                | Own threads for `A` and `B` read resolved (`status: fixed`). No new thread. Threads by others untouched.                                                                                              |
| 5   | `inspect_target` with PR 1's URL, then `watch_pull_request` (`expected` = saved repository, `account_generation: 0`).               | `inspect_target`: `provider: azure_devops`, PR number and the saved repository ID. `watch_pull_request` reuses queued/completed work for the unchanged head; `review_again: true` runs fresh.         |
| 6   | Repository gate: PR 2 adds only `outside/defect.md`.                                                                                | Not queued after a poll. `watch_pull_request` fails `paths_gate` and starts nothing, also with `review_again`.                                                                                        |
| 7   | Excluded-only: PR 3 edits only `probe/README.md`.                                                                                   | Not queued; `watch_pull_request` fails `paths_gate`.                                                                                                                                                  |
| 8   | On PR 1, push a change to `probe/data.txt` only.                                                                                    | Primary reviews the revision; secondary skipped (no `**/*.md` file changed since its last review). No new thread.                                                                                     |
| 9   | Then push a change to `probe/notes.md`.                                                                                             | Secondary runs on that revision; no new thread (no markers).                                                                                                                                          |
| 10  | Admission: a PR by an identity not in the include-users rule touching `probe/defect.md` (skip when only one identity is available). | Not queued. Record as unverified when skipped.                                                                                                                                                        |

## Cleanup

1. `save_repository` with `enabled: false`, then `value: null` to delete it.
   Delete the test agents and doctrine (`value: null`).
2. Abandon every test PR:
   `az repos pr update --id <n> --status abandoned --organization https://dev.azure.com/<org>`.
3. Delete topic branches: `git push origin --delete <branch>`.
4. Remove fixtures from `main` (or delete the disposable repository).
5. Stop the isolated app by PID; delete only its data directory and Keychain
   service.
