# PR Sniper Discovery Foundation

## Revision

- Revision: 8
- Subject: PR Sniper MVP feasibility and foundational architecture
- Rehydration mode: cold start
- Recovery state: foundation missing
- Alignment: verified by Dylan McCurry on 2026-09-18
- Lifetime: durable repository artifact

## Question and boundaries

Determine whether PR Sniper can be built as a practical tray-first application,
and gather enough evidence to choose its foundational desktop/runtime
architecture.

The first feasibility target is:

- GitHub first
- macOS first, while preserving a credible Windows path
- system tray/menu bar shell and settings
- scheduled monitoring while PR Sniper is running
- launch at login
- watched-author filtering
- reviewer-assignment triggers
- automated review through configurable locally installed agent systems
- machine sign-off followed by final human review
- machine-authored GitHub comments published through the signed-in human
  account and visibly signed by PR Sniper

The embedded diff reviewer, team leaderboard, Azure DevOps implementation, and
production release infrastructure are outside this first POC. They remain
product context and architectural constraints.

## Evidence inspected

### Product evidence

- GitHub issues 1 through 18 in `jdylanmc/pr-sniper`, created from the human's
  product description.
- Repository state at commit `6c0c13c`; the repository contains only the
  installed project-local agent skills.

### Official platform evidence

- Electron Tray:
  <https://www.electronjs.org/docs/latest/api/tray>
- Electron autoUpdater:
  <https://www.electronjs.org/docs/latest/api/auto-updater>
- Electron safeStorage:
  <https://www.electronjs.org/docs/latest/api/safe-storage>
- Tauri system tray:
  <https://v2.tauri.app/learn/system-tray/>
- Tauri shell plugin:
  <https://v2.tauri.app/plugin/shell/>
- Tauri updater plugin:
  <https://v2.tauri.app/plugin/updater/>
- GitHub App authentication:
  <https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/about-authentication-with-a-github-app>
- GitHub pull-request review endpoints:
  <https://docs.github.com/en/rest/pulls/reviews>
- Local GitHub Copilot CLI `--help` output from version 1.0.87-0.

### Local environment observations

- macOS arm64
- Node.js 24.20.0 and npm 11.19.0 are installed.
- Swift 6.4 is installed.
- Rust and Cargo were not found.
- GitHub Copilot CLI is installed at
  `/Users/dylan/.nvm/versions/node/v24.20.0/bin/copilot`.
- Copilot CLI exposes non-interactive prompts, model and custom-agent
  selection, working-directory selection, JSONL output, explicit session IDs,
  resume, and process cancellation.
- `claude` and `codex` resolve only to temporary cmux shims that fail their
  health check. `orca-cli` is absent.

## Confirmed findings

1. Electron and Tauri both provide official tray/menu-bar APIs.
2. Electron can use operating-system-backed encryption through `safeStorage`;
   consistent macOS signing matters for stable Keychain behavior.
3. Electron supports signed automatic updates on macOS and Windows.
4. Tauri provides an official tray API, permission-scoped child-process plugin,
   and updater that requires signature verification.
5. Tauri adds a Rust toolchain and native integration layer that are not
   currently present on this machine.
6. GitHub review APIs support pending reviews, inline comments, general
   comments, approvals, and change requests. Reviews must be bound to the
   correct commit/diff positions, and fast content creation can trigger
   secondary rate limits.
7. GitHub Apps distinguish app-attributed automation from user-attributed
   actions. The human selected user-attributed publishing for the initial
   product direction.
8. Polling is sufficient for the first GitHub MVP and avoids requiring a hosted
   webhook receiver.
9. A separate daemon is not currently required. Electron's main process or
   Tauri's Rust process can own scheduling while the tray application runs.
10. Launch-at-login satisfies the selected background-lifetime requirement.
11. Local agent discovery cannot trust `PATH` alone. It needs an executable
    probe plus a health/capability probe.
12. Each agent system should own its own authentication. PR Sniper should invoke
    adapters under the current user context rather than collect model-provider
    secrets by default.
13. A GitHub `APPROVE` event published with GitHub CLI credentials is attributed
    to the signed-in human and cannot honestly represent a distinct machine
    approval. The MVP therefore publishes comments only.
14. Pull-request content is untrusted input. Automatic review of a fork or an
    author outside the configured trusted watchlist requires confirmation, and
    the review path must not execute repository code, builds, tests, or install
    commands.
15. A pending GitHub review can batch inline comments before one visible
    submission. Revision and eligibility checks are required before and after
    every provider mutation; stale pending output is discarded, while a head
    change after visible submission stops further publication and requeues the
    new revision.

## Human decisions

- GitHub is the first provider.
- macOS is the first runnable platform; plan for Windows without over-indexing
  on it.
- Monitoring is required while PR Sniper is running in the tray, not after the
  application exits.
- PR Sniper should be configured as a login item.
- GitHub comments should be published through the signed-in human account and
  visibly signed by PR Sniper.
- The MVP must not submit a provider `APPROVE` event. Distinct automation
  approval remains a future GitHub App capability.
- Future provider approval policy uses explicit `off`, `manual`, and
  `automatic` modes rather than an ambiguous boolean.
- Review agents may read pull-request metadata, diffs, and repository source,
  but the MVP review path must not execute repository code, builds, tests, or
  install commands.
- Fork pull requests and pull requests from authors outside the configured
  trusted watchlist require confirmation before agent execution.
- Review comments are submitted as one pending-review batch after current
  revision, lifecycle, eligibility, and policy checks. Those checks repeat
  before and after every provider mutation.
- PR Sniper monitors unresolved threads that it created and may add a concise
  reply when it has meaningful new information. It does not produce filler or
  duplicate replies.
- Retryable operations honor provider timing, otherwise use exponential backoff
  with jitter, and stop after three retries or 15 minutes, whichever occurs
  first. The budget survives restart; exhaustion requires a visible manual
  retry.
- The agent runtime must be pluggable across available local systems.
- Run disposable Electron and Tauri POCs using the same acceptance probe.

## Meaningful routes

### Route A: Electron application

Hypothesis: Electron's main process can provide the complete scheduler, provider
adapter, agent adapter, tray, notification, settings, and update foundation
with the lowest initial implementation friction.

Strengths:

- mature tray and window APIs
- direct Node.js subprocess and scheduling ecosystem
- current machine already has the toolchain
- straightforward integration with local JavaScript/TypeScript agent adapters

Risks:

- larger package and idle memory footprint for a small utility
- renderer/main-process boundary must be hardened
- update behavior must be reconciled with Homebrew Cask and Chocolatey

Status: viable; requires measured POC evidence.

### Route B: Tauri application

Hypothesis: Tauri can provide the same product behavior with a much smaller
runtime footprint and a permission-scoped native backend.

Strengths:

- official tray, shell, and signed updater support
- explicit command permissions
- likely smaller package and idle footprint
- Rust backend is a credible long-term provider/scheduler core

Risks:

- adds Rust and more native implementation work
- local toolchain is currently absent
- agent process management and advanced desktop behavior may require more Rust
  code than the equivalent Electron implementation

Status: viable; requires toolchain setup and measured POC evidence.

### Route C: separate native clients plus shared daemon/core

Hypothesis: native macOS and Windows clients over a shared service provide the
best platform fidelity and process isolation.

Strengths:

- strongest native integration
- a daemon could isolate monitoring and review work from UI failures

Risks:

- two UI stacks and a process protocol from the start
- materially higher build, packaging, testing, and maintenance cost
- no current requirement for work after the tray process exits

Status: not selected for the first POC. Reopen only if Electron and Tauri fail a
hard requirement or process isolation becomes necessary.

## Scout application

- `scout / Principles / Name the decision before scouting it`: the decision is
  the foundational desktop/runtime architecture, not a general UI framework
  preference.
- `scout / Principles / Seek meaningfully different routes`: Electron, Tauri,
  and separate native clients/shared daemon expose different runtime and
  maintenance consequences.
- `scout / Principles / Set judgment before attachment`: compare the same tray,
  timer, notification, subprocess, cancellation, settings, packaging, startup,
  memory, and package-size observations.
- `scout / Principles / Buy the cheapest useful evidence`: disposable probes
  are cheaper and more discriminating than debating framework reputation.
- `scout / Boundary`: POC code remains disposable session evidence and does not
  become product code implicitly.

## Aligned domain model

### Actors

- Human reviewer: configures monitoring and provides final approval.
- Teammate author: opens or updates an eligible pull request.
- PR Sniper: detects, schedules, invokes, records, notifies, and signs machine
  review activity.
- Local review agent: an installed and authenticated agent executable invoked
  through an adapter.
- GitHub: source-control and review provider for the first MVP.

### Systems and boundaries

- Desktop shell: tray/menu-bar lifecycle, settings, queue, notifications, and
  login item.
- Monitoring engine: schedule, incremental polling, trigger evaluation,
  deduplication, and state transitions.
- GitHub provider adapter: authentication, repositories, pull requests, diffs,
  review threads, and publishing.
- Agent adapter registry: discovery, capability probing, invocation, structured
  result normalization, cancellation, and health.
- Local persistence: configuration, cursors, review jobs, and idempotency
  records.
- Operating-system security boundary: credential encryption/keychain and
  signing identity.

### Core states

- repository disabled or monitoring
- pull request ignored, eligible, queued, reviewing, waiting for author,
  waiting for human, machine-signed-off, human-complete, or failed
- agent unavailable, ready, running, cancelled, or failed
- provider unauthenticated, insufficient-permission, ready, rate-limited, or
  failed

### Core events

- scheduled check fires
- watched author opens or updates a pull request
- signed-in human is assigned as reviewer
- agent review starts, produces findings, requests human input, or signs off
- provider accepts or rejects a review mutation
- pull request revision changes
- notification is delivered or missed

### Important relationships

- A repository has provider, schedule, watch rules, agent selection, prompt, and
  automation policy.
- A pull-request revision produces at most one normalized review job per trigger
  policy.
- An agent adapter owns invocation mechanics but not provider publishing.
- The provider adapter owns confirmed remote state and never trusts a
  success-shaped local assumption.
- Machine sign-off is a recommendation; final human approval remains distinct.

## Frontier

### Ready for bounded POC

- Can Electron and Tauri each implement the same macOS tray lifecycle?
- Can each persist and execute a timer while no window is open?
- Can each show a notification and open a minimal settings window?
- Can each safely invoke Copilot CLI, capture JSONL, cancel it, and report
  failure?
- What are measured startup time, idle memory, package size, and implementation
  complexity?

### Needs later research

- Best production GitHub authentication flow for a local desktop app publishing
  as the user.
- Exact review-result schema across multiple local agent systems.
- Homebrew/Chocolatey coexistence with in-app primary/dogfood updates.
- Windows tray, notification, secure-storage, and packaging verification.
- Azure DevOps authentication and review capability.

### Needs later human input

- Exact canonical sniper emoji.
- Whether installation should require GitHub CLI/Copilot CLI or support a
  self-contained GitHub authentication path from the first release.

### Explicitly deferred

- embedded diff review overlay
- team leaderboard and hosted team state
- production-grade updater feeds
- Azure DevOps implementation

## Next action

Run a read-only GitHub provider probe in session storage. Verify authenticated
identity, repository polling, watched-author filtering, reviewer-assignment
queries, pull-request revision identity, changed-file retrieval, and existing
review retrieval without publishing or changing any remote state.

## Cycle 2 experimental evidence

### Prototype locations

- Electron:
  `files/poc-electron`
- Tauri:
  `files/poc-tauri`
- Isolated Rust toolchain:
  `files/rust-toolchain`

All locations are session artifacts outside the product repository.

### Tested behavior

Both Electron 38.1.2 and Tauri 2.11.x successfully:

- created a macOS tray/menu-bar process;
- created and hid a minimal settings window;
- persisted synthetic scheduled state;
- submitted a native notification;
- health-checked GitHub Copilot CLI;
- invoked Copilot non-interactively with JSONL output;
- terminated a child process;
- produced a macOS `.app` bundle.

### Measurements

| Observation | Electron | Tauri |
| --- | ---: | ---: |
| Ready-to-tray startup | 88 ms | 222 ms |
| App process count | 4 | 2 |
| Working set | 375,984 KB | 91,616 KB |
| `.app` size | 261,588 KB | 10,152 KB |
| Prototype source | 259 lines | 233 lines |
| Local dependency/build cache | 310,608 KB | 1,041,012 KB |

These are single-machine POC observations, not production benchmarks.

### Agent-output observation

A trivial Copilot JSONL invocation emitted 65 events. Forty-two were MCP server
status changes. The stream also contained user, turn, model, assistant delta,
final assistant message, usage checkpoint, idle, and result events. The result
record included an exit code, session ID, timestamp, and usage.

An agent adapter must therefore normalize an event stream and explicitly select
final result/message/usage records. Treating stdout as one model response is not
viable.

### Failures and fixes

- The initial npm commands ran from the wrong directory and made no prototype
  changes; rerunning from the isolated directories succeeded.
- The first Tauri npm CLI pin did not exist and was corrected to 2.11.4.
- Tauri required an isolated Rust toolchain.
- Tauri context generation required an RGBA application icon even for the
  minimal experiment.

### Experimental limitations

- macOS arm64 only
- notification submission was programmatically successful but not
  human-verified visually
- no Windows execution
- login-item APIs were not enabled or mutated; official Electron and Tauri
  documentation establishes support
- no GitHub pull-request writes
- no production signing or updater feed

## Cycle 2 aligned decision

Dylan verified the findings and selected Tauri for the next architecture step.

Rationale:

- Both routes passed the required behavior.
- Tauri's app was approximately 96% smaller.
- Tauri used approximately 76% less measured memory.
- The added Rust/toolchain complexity is acceptable for an always-running tray
  utility.

Electron remains a fallback if Windows validation or real agent process
management exposes material Tauri friction.

This satisfies `scout / Principles / Return when information stops paying` for
the desktop framework comparison. The selection is human-owned.

## Cycle 3 GitHub provider evidence

### Prototype

The read-only GitHub provider probe is retained at
`files/poc-github-readonly`.

### Observations

- The existing GitHub CLI credential authenticated as `jdylanmc`.
- A poll against `cli/cli` returned 20 open pull requests.
- Filtering to one selected watched author retained one pull request and ignored
  19 before any expensive review work.
- For `cli/cli#14475`, the probe retrieved:
  - head revision SHA;
  - 76 of 76 changed files;
  - four reviews;
  - eight review comments;
  - requested reviewers;
  - draft state.
- The revision-stable processing key
  `cli/cli#14475@9b6f66363f51a8265f1bb07f8fc3746017859cf1`
  is sufficient to identify one reviewed pull-request revision.
- Querying reviewer assignments for `jdylanmc` returned a valid empty result.
- Polling `jdylanmc/pr-sniper`, which had no open pull requests, returned a
  normal empty state rather than an error.
- No remote mutations were made.

### Rate-limit finding

GitHub's search bucket was 30 requests and decreased during the probe. The core
API bucket was 5,000. Repository polling or GraphQL with local cursors should be
the normal scheduled path. Global search should not run once per configured
repository on every schedule tick.

### Provider architecture finding

Use a provider abstraction with separate credential providers and API clients.
The initial GitHub credential provider may reuse GitHub CLI authentication, but
domain behavior must not depend on `gh` command output.

## Authentication direction

### Bootstrap path

For the first release, installed command-line credentials provide the lowest
friction:

- GitHub CLI credentials for GitHub;
- Azure CLI or `DefaultAzureCredential` for Azure DevOps.

These adapters let PR Sniper reuse an identity the user has already established.
They also make a guided Setup Doctor practical.

### Long-term standalone path

- GitHub: a GitHub App using user-to-server OAuth. GitHub documents that actions
  remain attributed to the user and are marked with the app identity.
- Azure DevOps: a Microsoft Entra application registration using delegated OAuth
  through Microsoft Authentication Library for an interactive desktop app.

Microsoft's current guidance names user-delegated Microsoft Entra OAuth as the
fit for interactive desktop applications, recommends it over personal access
tokens, and treats Azure CLI token acquisition as an ad hoc or development
route.

### Architectural consequence

Define a credential-provider interface from the start:

- GitHub CLI
- native GitHub App user OAuth
- Azure CLI / `DefaultAzureCredential`
- native Microsoft Entra delegated OAuth

Credential providers feed GitHub and Azure DevOps API clients. They are not the
provider domain contract. Tokens belong in operating-system secure storage.

## Setup Doctor decision

Add a Settings workflow that:

- detects required and optional command-line tools;
- health-checks executable resolution rather than trusting `PATH`;
- previews exact installation and sign-in commands;
- requires confirmation before running commands;
- opens or runs a controlled terminal bootstrap;
- streams progress and errors;
- verifies authenticated identity and effective capabilities afterward;
- remains idempotent;
- never embeds, prints, or stores credentials in scripts.

The Setup Doctor accelerates CLI-backed onboarding. It does not remove the
long-term native OAuth requirement.

## Cycle 4 agent-review evidence

### Prototype

The read-only agent-review probe is retained at `files/poc-agent-review`.

### Experiment

- Shallow-fetched public pull request `cli/cli#13788`.
- Materialized its head revision, pull-request metadata, diff, changed source,
  and tests in session storage.
- Invoked GitHub Copilot CLI against that checkout.
- Required one strict JSON result with:
  - concise synopsis;
  - ordered file guide;
  - structured findings;
  - machine decision.
- Validated the schema independently.
- Compared tracked repository state before and after the agent run.

### Observations

- The pull request changed one file with six additions and one deletion.
- Copilot produced one ordered file and one medium-severity finding.
- The result selected `human-review-required`.
- The agent changed zero tracked files.
- No provider mutation was made.
- Transcript audit showed only local `view`, `rg`, and `bash` calls.
- The finding stated that reading `RawPath` misses canonical escapes such as
  `%20` and recommended `EscapedPath()`.

### Independent validation

- An existing human review comment on the public pull request made the same
  diagnosis.
- Official Go `net/url` documentation states that `Path` is decoded and callers
  should use `EscapedPath()` when preserving original escaping matters.
- The agent transcript did not retrieve the existing review discussion, so the
  matching diagnosis was independently derived from source and diff context.

### Result

The core machine-review seam is feasible:

1. detect a pull-request revision;
2. materialize revision context;
3. invoke a local agent;
4. normalize synopsis, file order, findings, and decision;
5. keep provider publication as a separate confirmed operation.

Confidence: 9/10 for MVP feasibility.

## Final frontier

### Ready for specification

- Tauri-based macOS-first tray application
- startup/login-item behavior
- configured GitHub repositories
- scheduled repository polling
- watched-author and reviewer-assignment eligibility
- head-SHA revision deduplication
- pluggable local agent adapters
- structured machine-review output
- human-account comment publication with PR Sniper signature
- human handoff and desktop notification
- guided prerequisite and sign-in Setup Doctor
- provider and credential abstractions that allow native OAuth later

### Deferred or needing later evidence

- Windows runtime and packaging validation
- Azure DevOps API and permission parity
- native GitHub App OAuth implementation
- native Microsoft Entra delegated OAuth implementation and app registration
- multi-agent normalized-schema compatibility
- pull requests with more than 100 changed files
- conditional requests, GraphQL optimization, and enterprise GitHub
- review lifecycle/idempotency edge cases
- production code signing, notarization, primary/dogfood updater feeds
- embedded diff review and team leaderboard

## Final aligned next action

Dylan verified the cycle 4 findings and selected Specification as the next
workflow. Tracker maintenance was explicitly approved for:

- a Setup Doctor issue;
- updates to issues 5 and 6 recording the bootstrap and long-term authentication
  direction.

Before specification, Dylan settled the remaining product choices:

- Launch at login is opt-in.
- `` is the canonical crosshair application icon and brand mark.
- ` PR Sniper` is the canonical machine-review signature.

The independent specification review surfaced further product ambiguities.
Dylan settled them as follows:

- Review automation has two independent per-repository gates:
  - start the local agent automatically;
  - publish machine comments automatically;
- The effective agent-start gate is checked immediately before invocation. If
  automatic start was disabled after queueing, explicit human start is required.
- If the pull-request head changes before publication, discard the unpublished
  result and queue the new revision. Never knowingly publish stale comments.
- The configured local review agent is trusted with the current user's access.
  PR Sniper does not claim to sandbox the agent away from existing GitHub or
  Azure credentials. PR Sniper still keeps its own validated publication
  operation separate and never instructs the review agent to publish.
- The review guide orders every changed file, including pull requests with
  hundreds of files. The interface may progressively disclose the list, but it
  must preserve complete coverage and provide rich repository context to the
  agent.

The second independent specification review exposed approval-identity,
untrusted-input, publication-race, retry, and conversation-loop gaps. Dylan
settled them as follows:

- The MVP publishes comments only and never submits a provider `APPROVE` event.
  Automated approval is deferred until PR Sniper has a distinct GitHub App
  automation identity.
- A future approval policy has explicit `off`, `manual`, and `automatic` modes.
- Pull-request content is untrusted. Review agents receive read-only metadata,
  diffs, source, and search context; the review path does not execute repository
  code, builds, tests, hooks, or installation commands.
- Fork pull requests and authors outside the configured trusted watchlist
  require confirmation before agent execution.
- An untrusted pull request is blocked when its adapter cannot enforce the
  read-only tool policy. Confirmation may accept the trust classification but
  never bypass the tool restriction.
- Inline comments become visible through one pending-review `COMMENT`
  submission. PR Sniper revalidates revision, lifecycle, eligibility, trust,
  repository state, and effective gates before and after every provider
  mutation.
- A revision change before visible submission discards the pending review and
  requeues. A revision change after visible submission stops further
  publication, records the partial stale state, and requeues the new revision
  only when it remains eligible. Eligibility loss without a new revision stops
  publication and does not requeue ineligible work.
- PR Sniper monitors unresolved threads it created and replies only when there
  is meaningful new information. Replies remain concise, avoid filler and
  duplication, and defer questions requiring human judgment.
- A follow-up job is keyed by the owned thread, triggering external comment,
  and reviewed head revision. Polling and restart may produce at most one reply
  for that key; a later external comment creates a new eligible follow-up.
- Automatic retry applies only to provider rate limits, transient network or
  transport failures, provider server failures, transient checkout fetch or
  input/output failures, and agent launch, timeout, or temporary-exit failures
  that occurred before any provider mutation.
- Authentication, permission, configuration, cancellation, schema validation,
  stale revision, ineligibility, and explicit provider rejection require
  correction or human action rather than automatic retry.
- Each persisted job-operation identity receives one budget. The initial
  attempt starts its 15-minute deadline and may be followed by at most three
  retries. Provider timing takes precedence; otherwise retry uses exponential
  backoff with jitter.
- Restart preserves the attempt count and deadline. An expired deadline enters
  manual-retry state without another automatic attempt. Explicit manual retry
  creates a new budget.
