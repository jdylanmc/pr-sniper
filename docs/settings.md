# Settings

Reference for Settings navigation, accounts, repositories, assignments, Agents, Doctrines, Voices, Preferences and the onboarding Genie.

## Layout

- Overview: saved resource counts and a Genie icon by the title.
- Two grouped cards. **General**: **Accounts**, **Repositories**, **Preferences** (AI capacity lives there). **Review behavior**: **Agents**, **Doctrines**, **Voices**.
- Back restores the row and scroll. Back never saves or resets other drafts.
- Collections (Agents, Doctrines, Voices) are one grouped list: `.resource-library` card, hairline-separated `.resource-card` rows. Counts are singular-aware.
- Resource editors keep Save/Cancel. No global save bar. Lists have no save bar.
- Reset discards remaining unsaved edits, not successful resource saves. Controls and dialog dismissal are locked during saves.
- Dialogs keep focus, `Escape`/Close and background isolation without native dialog APIs.
- Standalone Settings (menu **Settings**) keeps the combined Integrations editor.

## Accounts

- **Accounts** → **AI Tooling** (GitHub Copilot) and **Git Repository** (GitHub). Back goes one level up. Genie opens a provider directly. Pending sign-ins survive navigation.
- GitHub identity (repos, publication) is separate from Copilot identity (AI). Connecting one never signs in the other.
- Buttons: **Add GitHub account**, **Connect Copilot account**. Device-code waiting, failure and retry states are shown. Identity is saved when browser sign-in completes. Cancel does not connect.
- Connected row has a disconnect icon; it forgets the account.
- **Reconnect** and sign-in again use the same device-code flow; the code is shown before the browser opens. A sign-in whose provider account ID matches a saved account renews that account's credential in place and clears **Reconnect required**; its parked credentials failures resume.
- Connecting does not assign repos, models or automation.
- GitHub connect uses the PR Sniper GitHub OAuth App, scope `repo` ([ADR-0003](adr/0003-github-browser-oauth.md)). Verification and PR reads: [polling.md](polling.md).
- Disabled future providers: Claude, Codex, Grok. **Coming soon**: Azure DevOps, Bitbucket.
- Copilot catalog and account settings: [copilot-settings.md](copilot-settings.md).

## Repositories

- Compact drill-in rows per configured repo. Status: **Needs setup**, **Enabled**, **Paused**, **Reconnect account**. No bulk toggles. Repos of disconnected accounts sit in a bottom group.
- Add by magnifier: connected GitHub account → personal owner or org with accessible repos → search. Org list derives from the accessible-repo catalog, not full membership. Managed usernames (enterprise underscore suffix) work.
- Paginated GitHub reads keep successful results on SSO, malformed or later-page failure and show a boundary-specific warning. Never shown complete or empty on failure. **Retry** refreshes owners and the selected owner's repos; selection and search stay.
- Session or credential loss, changed connection or unusable authorization clears that account's cached owners/results until fresh lookup. Unusable credential blocks native Save.
- Missing `repo` scope: explicit reconnect. Org may need admin approval. No retry swaps account or uses GitHub CLI login.
- **+ URL**: GitHub URL or `owner/repository`, explicit acting GitHub account, stable repo identity validated. AI accounts are not repo accounts.
- Selecting adds a durable disabled, unassigned row and opens its config. Leaving before Save keeps the disabled row. No repo-count limit.
- Save writes admission rules and assignments; first valid Save enables monitoring. Rules: [polling.md](polling.md). Existing repos migrated to filters with monitoring off.
- Back keeps the repo draft for the session. Cancel discards only that repo's unsaved changes. Back/Cancel asks before discarding unsaved assignment or nested Agent fields.
- Closing the repo editor cancels pending draft lookup. Owner, account or route changes fence async results. Failed reads/saves keep errors and retry; no unbound success fallback.
- Unbinding: Save or Cancel repo changes first. Unrelated Preferences drafts do not block.

## Assignments

- Assignment editor drill-ins: **Agent**, **Permissions**. Primary switch. **Approval** pill: `off | Approve | & Merge` (primary only).
- Permissions: Publish Comment (automatic initial publication), Reply Comment (primary), Approve, Merge. New assignments start with an explicit Agent choice and every grant off. Existing grants unchanged.
- From an assignment: **Edit Agent** opens the shared Agent; **Create new Agent** works with an empty library. Save/Cancel returns to the same assignment draft. New Agent starts without permissions. Unavailable Agent refs stay explicit until replaced.
- Grants are repository-assignment controls, never global Agent permissions. Mechanics: [provider-writes.md](provider-writes.md).

## Agents and Doctrines

- Compact libraries, one-level editors. No cap on saved Agents; capacity limits concurrent work.
- Agent = explicit Copilot account + real model that account returns + ordered list of 0..many Doctrines + prompt + signature. Provider and model are distinct. Single-doctrine selections stay compatible; empty list = none. Bodies compose in selected order, not library order. Missing, unavailable or legacy selections stay visible, never silently replaced. Agent editor shows a reconnect-required row with **Fix**.
- Prompts, principles and signatures are editable. Never put credentials in doctrines, prompts or config fields.
- Old Agents without AI account binding stay unconfigured until updated. Disconnecting an AI account keeps dependent Agents and assignments.
- Renaming a Doctrine updates references atomically. Referenced Doctrines and Agents cannot be deleted until references are repaired.
- Doctrine checklist: filter and scroll by mouse or keyboard, 0..many selected. Filtering keeps hidden selections.
- Save applies that resource only. Back/Cancel discards that editor's unsaved fields only.
- Doctrine editor shows sharing users; delete refused while referenced. Unreferenced delete is confirmed and keeps completed review evidence.
- Fresh config persists ten bundled Doctrines on first load, before any Settings tab is visited. Catalog `src-tauri/doctrines` (embedded; no download at runtime) is independent of `.agents/skills/doctrine`. Defaults target 250 words, 500-word ceiling; ceiling does not bind user text.
- Edits, additions and deletions (even deleting the whole library) survive restart. Libraries are never topped up or replaced. Legacy settings without a doctrine field stay empty; next save writes an explicit empty list.

## Voices

- Library like Doctrines: list drills into an editor; floating **New voice**; trash icon per row; delete refused while an Agent uses it; rename updates Agents.
- Voice = title + style text + **Fixed guardrails** checkbox (default on). Label: "Fixed guardrails: tone only; critique code, never people".
- Seeds: Caveman, Gilfoyle, Merlin (`src-tauri/voices`, embedded). Added once to a library that never held Voices (`voice_catalog_version` absent); never topped up. Deleted seeds and an empty library survive restart.
- Agent editor: **Voice** row drills into None + library (radio). Default None. Missing selection stays visible as unavailable until changed.
- Affects only primary text for humans. Packet rules: [ADR 0022](adr/0022-voices.md).

## Preferences

- Saved: one global cron + timezone, machine AI capacity, comment-publication defaults.
- Immediate (native, outside Save/Reset): notification opt-in, startup. Pause lives only in the panel header **Monitoring** switch.
- AI capacity: integer `1..4294967295`, default 4. Shared by normal, final-primary, reply and mention work. Admitted jobs drain independent of polling.
- Cron helper shows the exact expression. Time zone: own row, dropdown of host IANA zones (`time_zones`, the list `Schedule::validate` accepts); host zone first. A saved zone missing from the list stays selected, marked unavailable. A brand-new profile starts in the host zone (valid `TZ`, else the `/etc/localtime` link; Windows/unknown: UTC); existing saved zones never change. No silent timezone or legacy-interval conversion. One schedule: [polling.md](polling.md).
- Native controls keep pending/rejected ops across saves.
- Startup shows the saved request, separate from actual registration. Unknown registration or notification authorization is never shown as enabled.
- macOS startup status validates the owned plist and current executable. macOS Login Items may still disable it. App start only inspects; never enables, disables or reapplies.
- Windows startup: [windows-development.md](windows-development.md), [ADR-0006](adr/0006-windows-tray-startup.md).
- **Copy diagnostics** (content-free export to clipboard). Notifications: [review-queue.md](review-queue.md#notifications).

## Onboarding Genie

- Empty install opens **Welcome** with **Set up with Genie** and **I’ll set it up myself**. No invented data. Genie also opens from the Settings title icon after setup.
- Genie opens the same editors as Settings. Each save is immediate. Closing hides the editor with no rollback. Unsaved fields are lost on quit. Reopen never resets libraries or monitoring.
- Four readiness rows come from saved resources and native account/scope state, not visited steps.
- Setup summary: both identities, effective assignments, primary and permissions, admission filters, schedule and timezone, AI capacity, global pause. It checks assigned models against catalogs, not subscriptions or inference.
- Back cancels summary preflight; it cannot undo a dispatched repo save. **Finish setup** only leaves the summary. Failed writes keep saved resources; retry.
- Global pause is never resumed implicitly. All repos disabled, or no bindings/assignments, means intentionally inactive; the Queue stays empty.
- A late onboarding read never replaces newer navigation. Repo and AI connection state fence setup reads.
