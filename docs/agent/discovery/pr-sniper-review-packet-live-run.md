# Review packet: live run (M7)

- Date: 2026-10-13. Branch: `feat/packet-live`.
- Sandbox: `jdylanmc/pr-sniper-sandbox` (private, owner-authorized). PR #1 clean docs, #2 bug (`average` off-by-one, `os.system`), #3 CI red.
- GitHub account: `jdylanmc` (author of all three PRs: every PR is self-authored).
- Copilot account: separate work account, token mode (`jdylanmc` has Copilot Free; its model list is `auto` only and `auto` returns HTTP 400).
- Models: `gpt-5-mini` (development runs), `gpt-5.4-mini` (final runs).
- Harness: `src-tauri/src/live_tests.rs`. Every test `#[ignore]`, returns early unless `PR_SNIPER_LIVE=1`. CI never runs it.

```sh
PR_SNIPER_LIVE=1 PR_SNIPER_LIVE_MODEL=gpt-5.4-mini PR_SNIPER_LIVE_STORE=<dir> \
  cargo test --locked --lib live_tests -- --ignored --nocapture --test-threads=1
```

## Real code driven

| Step | Production code |
| --- | --- |
| GitHub over HTTPS | `GithubClient<HttpTransport>` from a token pair |
| State read, hash | `pr_snapshot`, `normalize().hash()` |
| Heartbeat | `PrStates::observe` (new iteration, pending, baseline) |
| Feedback poll | `owned_threads_at` + `Ledger::observe` |
| Queue, dispatch | `review::host::request`, `prepare_dispatch` (temp `Store`) |
| Packet | `packet_run::prepare` (`Caller`, teammates via `teammates::entries`) |
| Agent | `review::runtime::run` (bundled Copilot runtime, token mode, real tools, terminal exits, exit gate) |
| Completion | `review::host::complete` (`honour_local` for `ready_for_human`) |
| Comments | `publication::execute` -> `Batch::prepare` -> `prepare_exit`, `mutate_publication`, `reconcile_publication` |
| Replies | `exit_writes::run` (preflight, `reply_to_review_thread`) |
| Consume | `exits::consume::consume_after` / `settle` |

Replaced by thin test environments (same client calls): tauri `Host` locks, account generations, monitoring activation, backpressure pacing, `before_send` remote gate (allow).

## Results (final runs, `gpt-5.4-mini`)

| Run | Exit | Tools | Packet | Tokens in/out | Time |
| --- | --- | --- | --- | --- | --- |
| a teammate #2 | `submit_report`, 2 findings | read_changes, read_source x2 | 2240 B | 4350/1852 | 25 s |
| b primary #2 | `leave_comments`, 2 new | read_changes, read_source x2 | 3659 B | 3926/438 | 21 s |
| e teammate #2 | `submit_report` | read_changes x3 | 3377 B | 3592/863 | 23 s |
| e primary #2 | `leave_comments`, 2 replies | read_changes, read_source | 4846 B | 7016/780 | 22 s |
| c primary #3 | `ready_for_human` | read_changes x2, get_checks | 2440 B | 7266/1006 | 18 s |
| d primary #1 | `ready_for_human` | read_changes | 2473 B | 3056/191 | 13 s |

Proven:

- (a) Teammate ends on `submit_report`. Findings caveman: `calc.py:5` "average skips the first value, so every result is too small; one-item lists return 0." `calc.py:10` "run() shells out with os.system; ... command injection."
- (b) Primary packet carries the Teammate reports section. Exit `leave_comments`. Publication `Published`, 3 receipts. GitHub API: comments on `calc.py:5` and `calc.py:9`, side RIGHT, lines equal to the request. Excerpts: "Bug: this loop skips `values[0]`, so every non-empty average is too small..."; "Security issue: `os.system(cmd)` invokes a shell...".
- Consume: `consume_after` baselines the post-write hash. Next heartbeat: `Change::None`, baseline == current hash. Own writes do not retrigger.
- (e) Human-like reply (as `jdylanmc`, so author is the acting account) on the agent's thread: heartbeat `Pending(3)`. Feedback poll observed. Second primary sees the thread at `calc.py:5`, `new_since_last_review=true` (only that thread). Exit: 0 new, 2 replies. `exit_writes::run`: preflight ok, both replies `Done` (in reply to the agent's roots). Consume after replies: heartbeat `Change::None`.
- (c) PR #3: approve not offered, packet says "CI is red. Do not approve. Say so with the fix." No approval on GitHub. Agent: `ready_for_human`, summary "CI is red: 2 checks failed, so this cannot be approved..."
- (d) PR #1: approve not offered, "Acting account authored this PR. GitHub refuses self-approval." `ready_for_human`. No GitHub write (comment and review counts unchanged). Baseline = read hash. Heartbeat `Change::None`.
- Teammate exits: only `submit_report`, `panic_exit`. Primary: no `submit_report`.
- No approve or merge was attempted on GitHub: gates never offered them (self-authored).

## Bugs found live, fixed

| Bug | Effect | Fix | Test |
| --- | --- | --- | --- |
| Tool handlers ran blocking `reqwest` on the async runtime worker | Debug builds: first `read_source`/before/after read panicked in the worker; run hung until the 600 s timeout. Release: a worker stalls per call. Exit gate (HTTP) same | `runtime::off_runtime` (`block_in_place` on the multi-thread review runtime) around `ReadTools` and `ExitTools` calls | `tool_handlers_run_blocking_http_off_the_async_worker` |
| Self-authorship denial masked CI red and conflicts | Packet on a red self-authored PR said only "self-approval"; agent never learned CI was red | Gate order: conflicts, CI red before self-authorship (approve and merge) | `self_authorship_never_masks_an_actionable_pr_state_reason` |
| Thread `path`/`line` never filled | Packet showed `thread ... at -`; agent could not place threads | Observation query reads `path line originalLine`; `reads::conversation` fills root location | `actions::tests` packet fixture asserts `at src/lib.rs:3` |
| `new_since_last_review` always `false` | Human replies after a review looked old | `PrStateRecord.consumed_at` set on consume; `reads::stamp_new_since` compares comment `updatedAt`; `Caller.last_consumed_at` | packet fixture + `consuming_records_when_and_a_new_head_keeps_it` |
| Unknown thread id accepted at exit | Model mangled one id (`...9rtc6...` for `...9wc6...`); preflight failed with `IncompleteRead`, whole exit tossed after the run | Exit gate denies replies/resolves naming a thread not on the PR; agent retries in the loop | packet fixture: `mapped-typo` denied |

## Not proven live

- Thread resolution: no run chose `resolve_own_threads`. Unit tests only.
- Approve and merge effects (`actions/packet.rs`): gates denied them on self-authored PRs (expected). A non-self PR is needed.
- Tauri glue: `Host` locks, generations, Coordinator pumps, monitoring activation, `pace_write`, `before_send` remote gate, diagnostics persistence.
- Real heartbeat poll scheduling and queue (jobs built directly; `observe` and `Ledger::observe` called by hand).
- Backpressure (no rate limit hit), panic exit, toss-and-requeue after a failed write.
- Packet view and Reviewed Clean UI.

## Observed agent behavior (not fixed, product question)

- `gpt-5-mini` second primary re-posted the same findings as new comments next to its own open threads (location now shown). Rules have no "do not repeat existing threads". `gpt-5.4-mini` replied instead.
- Agents sometimes post a "what I read" inline comment on line 1.
- PR #3: agent read file `FAIL` as a status and said the packet had no path.
- With CI red, both models chose `ready_for_human` with a CI summary, not a comment with the fix.

## Sandbox state

- PRs #1-#3 open, unmerged, no approvals.
- PR #2: final run comments stay as evidence (2 roots, 1 human-like reply, 2 agent replies). Earlier development runs' comments deleted; their empty `COMMENTED` review shells remain (GitHub cannot delete submitted reviews).
- Harness note: an early harness published comments for an exit whose replies were tossed; production holds that batch (`holds_publication`). Harness now checks the hold; those comments were deleted.
