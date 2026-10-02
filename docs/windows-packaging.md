# Windows installer and Chocolatey candidates

The unsigned installer and private Chocolatey package were delivered in
[#87](https://github.com/jdylanmc/pr-sniper/pull/87), completing the local package
scope of [#63](https://github.com/jdylanmc/pr-sniper/issues/63). This is **not a
trusted public Windows release**. Signed Windows releases
([#62](https://github.com/jdylanmc/pr-sniper/issues/62)) and public Chocolatey
publication ([#64](https://github.com/jdylanmc/pr-sniper/issues/64)) remain open
and explicitly deferred by the owner.

## Delivery status

**Status as of 2026-10-01:** The Windows app and private packaging are delivered;
public distribution remains deferred. The **Windows native application** workflow
was temporarily disabled to unblock macOS work, not because the code was removed.
Its hardened configuration retains both `windows` and
`windows-installer-acceptance` jobs. The operational enable/disable switch lives
in GitHub; verify its current state rather than inferring it from the YAML.
macOS CI and signed macOS release workflows are independent and unchanged.

The completed delivery is anchored to merged commit
`513c1a3f5746450dfc3022a988634577901ed97e`:

- [Post-merge Windows CI](https://github.com/jdylanmc/pr-sniper/actions/runs/36811769429)
  passed application/build checks and the full hosted installer, upgrade,
  uninstall, fault/retry, preservation and cleanup acceptance.
- [Post-merge macOS CI](https://github.com/jdylanmc/pr-sniper/actions/runs/36811769439)
  passed at the same commit.
- [Authorized local VM verification](https://github.com/jdylanmc/pr-sniper/pull/87#issuecomment-5924655151)
  separately proved actual tray/menu behavior, upgrade/uninstall, interrupted
  removal recovery and clean reinstall. The original app and profile were
  restored; hosted process readiness alone is not interactive GUI proof.

These receipts establish acceptance for that delivery snapshot, not every later
`main` commit. Public Windows release preflight still requires a green Windows
**main-push** run; an operational pause does not waive that gate or justify
reusing an older pass for a newer release.

### CI execution and troubleshooting

The workflow runs all checks on pull requests and `main` pushes, and accepts
manual `workflow_dispatch` runs. Superseded runs of the same PR are cancelled;
`main` runs are not automatically cancelled, preserving exact-commit release
evidence. Queue time is separate from execution time and is not a test hang.

The pinned Rust cache action restores compiler/lockfile-keyed dependencies.
Only successful `main` pushes save caches; PRs and manual runs only restore.
Workspace crates are rebuilt, and only Cargo's debug/release `.fingerprint`,
`build` and `deps` directories plus registry/git dependencies are cached.
Installer artifacts, extracted payloads, upgrade-fixture directories and
acceptance profiles are outside the cache paths.
A cache miss still runs the full cold build and every check; it is not a bypass.

The build job retains a 60-minute limit and installer acceptance a 15-minute
limit, with shorter timeouts on individual command steps. Every actual Chocolatey
test command prints a start message, expectation and elapsed-time result.
Deliberate faults are labelled as expected nonzero exits; their raw output is
retained in separate `choco-*.log` files in the installer diagnostics artifact.
An unexpected zero/nonzero exit prints the log tail and fails the check.
Expected failure is not enough to pass: the existing independent ownership,
preservation and recovery assertions still run.

To resume after an intentional operational pause:

```powershell
gh workflow enable windows.yml --repo jdylanmc/pr-sniper
gh workflow run windows.yml --repo jdylanmc/pr-sniper --ref main
```

Confirm the actual run conclusion before claiming Windows is healthy. A manual
run is useful for diagnostics but does not replace the required `main`-push
release evidence.

## Installer contract

Installer fixes serialize this current-user installation using a SID-scoped
cross-session mutex, held across interactive pages and mutation. Owner/version
and shortcut approvals are rechecked at the mutating section and after payload
staging. A waiting old installer cannot downgrade a concurrently upgraded app.
WebView2 machine-miss/zero-version fallback has its own handled error scope.
Post-staging validation refusals (including the old app starting during extraction)
enter the existing rollback and remove only this operation's claimed stages.
The original refusal is retained even if cleanup also fails. Pre-existing foreign
staging paths still cause an untouched refusal before that cleanup route is armed.

Candidate application, uninstaller and shortcut are staged before replacement.
Only exact old owned files are moved to backups; all ten owned registration
values must match the finite typed schema before their prior version is saved.
Writes/deletes check individual native return codes. Pre-commit failures restore
those exact old files, shortcut and registration values; unknown values are not
snapshotted or overwritten. A rollback or cleanup failure is nonzero and retains
recovery evidence in `.pr-sniper-transaction` and the explicitly named shortcut
backup. Do not remove these paths blindly: reconcile the exact recovery receipt
and retained files before retrying. Process termination/power loss is not claimed
to be a crash-atomic transaction.

Registry string payloads are passed to the NSIS System plug-in through a Unicode
register source, with the UTF-16 byte count including its terminator computed
from that same value. Literal quotes in `DisplayIcon` and uninstall commands must
not be interpolated into the plug-in's call-description syntax. Hosted acceptance
checks those three quoted `REG_SZ` values after installation and upgrade.

Uninstall likewise rolls back failed owned removals. Chocolatey independently
checks app absence, no pending transaction, all ten owned registry values absent
and no remaining owned/unresolved shortcut before removing the receipt-verified
uninstaller. This cleanup holds the same lifecycle mutex after native exit.
Foreign values/nonempty containers survive; in-place `_?=` self-deletion is
intentionally deferred, not treated as a failed native removal.

Shortcut inspection distinguishes absent, readable-owned, readable-foreign and
unreadable/unresolved. COM load/path failures are not foreign ownership: native
uninstall checks before destruction and again before moving the shortcut, and
aborts or rolls back on an unreadable result. An empty owned uninstall key must
actually disappear before backups are discarded; a Delete-only denial restores
the prior registration/files. A nonempty foreign key is retained instead.

At each package installation, `installation.json` gets a new random installation
ID, so even a same-version/same-binary reinstall has distinct receipt bytes.
Two exclusively created, flushed, immutable records are
enrolled in Chocolatey's installed-file snapshot: schema-2 `native-removal.json`
binds the exact installation receipt bytes, version, directory and uninstaller
hash; `native-removal.pending.json` records **not completed** with the same binding.
Only after native uninstall returns zero does the wrapper exclusively create and
flush a bound completion receipt at
`%ChocolateyInstall%\lib-bad\PACKAGE\native-removal-RECEIPT_SHA256.json`, then
validate and delete the pending marker. This single file is outside Chocolatey's
replaceable versioned failed copy, but inside its package-specific success
cleanup. Existing files are validated, never overwritten; neither directory
scanning nor foreign completion adoption is used.
The valid immutable state record plus committed pending-marker
absence means **native removal completed**. The state record itself is never
rewritten or deleted by the uninstall script.

This matters because Chocolatey 2.7.4 removes only tracked paths whose checksum
still matches: both newly created-at-uninstall and modified records can otherwise
survive a successful package removal. On success Chocolatey owns final deletion
of the unchanged state/installation records. If later executable or outer cleanup
fails, retained evidence still permits retry under the SID mutex and **all** live
removal postconditions; native uninstall is not repeated. Restored pending markers
are reconciled only after those locked checks, including any remaining
uninstaller's original hash. The durable receipt itself is never removed by our
script, even after executable deletion.
If executable deletion succeeded but Chocolatey's outer package cleanup failed,
the same bound receipts also permit an already-absent uninstaller: revalidate
both receipts, current absence and live removal postconditions under the SID
mutex, then finish without launching or deleting a nonexistent file. A present
file still requires its original hash; absence without completion evidence fails.
Absent registration alone never admits resume. Missing/corrupt completion
evidence, changed hashes, a new installation, an empty leftover key or pending
transaction fail closed; do not manufacture a receipt. Legacy untracked schema-1
completion files are not adopted or deleted automatically. Reconcile/archive
their exact evidence before installing regenerated packages. Earlier schema-2
packages without a unique installation ID also require explicit reconciliation;
they cannot gain a durable receipt by guessing that absent registration means
success. Persistence/marker
commit failure or interruption with uncertain phase still requires reconciliation.
The now-empty application directory may remain; no extra fallible directory
cleanup follows deletion of the last recovery executable.

**Failed-uninstall rollback:** Chocolatey 2.7.4 moves a failed package tree
to its versioned `lib-bad` location, then restores the pre-operation backup into
`lib`. Actual native reproduction confirmed that this resurrects the original
pending marker after successful native removal and failed executable deletion.
A subsequent failed attempt can also replace the versioned failed copy, so that
copy alone is not durable proof. The sibling completion receipt survives both
operations, including a retry refused by a busy lifecycle mutex. A validated
receipt is evidence of the earlier native success, not permission to bypass
current ownership checks. Chocolatey's successful `UninstallCleanup` removes the
package's `lib-bad` tree; unchanged active records remain eligible for its normal
installed-file cleanup. See the pinned
[failure/success handling](https://github.com/chocolatey/choco/blob/2.7.4/src/chocolatey/infrastructure.app/services/ChocolateyPackageService.cs#L1507-L1575)
and [versioned replacement](https://github.com/chocolatey/choco/blob/2.7.4/src/chocolatey/infrastructure.app/services/FilesService.cs#L186-L251).

Retain the exact active installation/state receipts, matching durable receipt
and original uninstaller if present; retry the same package's normal uninstall
after resolving the file lock/read-only condition. Do not use force, manufacture
completion, delete markers to claim success, or reconcile a newly installed app
using an old receipt. Missing/corrupt proof, denied persistence, partial package
cleanup or changed live state requires exact operator reconciliation. This is
not a crash-atomic ledger or a general repair framework.

The retained-state assertion and read-only fault case remain mandatory. Pure
tests cover repeated backup restoration and stale/foreign binding refusal;
real Chocolatey fault/retry and residue-free cleanup passed for the
[recorded delivery](#delivery-status). Changes to this protocol still require
fresh native validation; clean install/uninstall/reinstall alone does not
establish the failure path.
`removal-after-chocolatey-failure.json`, in the existing failure-retained
diagnostic artifact, records expected/observed app presence, exact uninstaller
hash, completion status and the three exact receipt-file hashes/presence in
active, same-version `lib-bad` and `lib-bkp` trees, plus the durable receipt's
presence/hash. It contains no registry or credential dump and grants no
recovery/adoption authority.

The pinned Tauri CLI 2.11.4 bundles the existing x64 GUI application using
`tauri.windows.conf.json` and `src-tauri/windows/installer.nsi`. This small
Tauri-supported NSIS template deliberately avoids the upstream template's
process-name termination, optional application-data deletion and unconditional
product-name Run-value deletion. It is not an application updater.

- Identity stays `com.jdylanmc.pr-sniper`, product `PR Sniper`. All checked-in
  version fields still agree; this change does **not** select a new release.
- One current-user location: `%LOCALAPPDATA%\PR Sniper`. No elevation request,
  machine install, directory selection, desktop shortcut or second side-by-side
  installation. `/D=` to another location is rejected.
- Requires **Microsoft Edge WebView2 Evergreen Runtime**, already installed for
  the machine or invoking user. Missing runtime fails before application files
  are copied. No bootstrapper, unrelated dependency, trust root or Windows
  security setting is installed/modified. Obtain WebView2 from
  [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/).
- Install: `pr-sniper-VERSION-x64-setup.exe /S`. Uninstall:
  `"%LOCALAPPDATA%\PR Sniper\uninstall.exe" /S`. Switches are case-sensitive.
  Success is **0 only**; cancellation/failed preconditions fail. Do not treat
  MSI reboot codes as success for this NSIS contract. Quit through the app's
  tray first; installers refuse a running host instead of terminating it.
- Installation never launches the app or opts into startup, notifications,
  review, publication or provider sign-in. Interactive installation likewise
  has no automatic-launch finish checkbox.
- Upgrade keeps the same path and requires this installer's matching marker,
  location and uninstall command. A manually copied application or foreign
  registration is not silently adopted. Downgrade/unknown version fails.
- The current user's `Programs\PR Sniper.lnk` must be absent or target this
  exact executable. Uninstall removes it only when that target still matches.
  A stale uninstaller refuses a different installed version.
- Only `pr-sniper.exe`, `uninstall.exe` and the template's enumerated values in
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper` are
  installer-owned. Unknown files, values and subkeys survive; directory/key
  removal is empty-only, never recursive. Resources/sidecars require an
  explicit template review, not automatic broad deletion.

**Settings, state, diagnostics and Windows Credential Manager records survive
both upgrade and uninstall.** This includes `%LOCALAPPDATA%\com.jdylanmc.pr-sniper`
and custom profiles. No credential enumeration or application-data purge occurs.
The uninstaller deliberately does not alter **any** native startup/notification
registration, including another installation's records.

Before permanent removal, an operator who previously opted in should turn off
**Launch at login** in the actual app's Settings, then Quit. If notification
identity removal is wanted, use the exact existing owner-token unregister
procedure in [Windows notifications](windows-notifications.md#explicit-removal-and-isolated-test-cleanup)
while that exact executable is still present. Notification opt-out alone is not
identity removal. No installer invents a token, reads credentials, scans/deletes
CLSID trees or impersonates another installation. Without this explicit step,
the registrations are retained (possibly pointing to an absent executable)
for deliberate operator reconciliation or a same-path reinstall.

## Build and stage without installing

Use [Windows development prerequisites](windows-development.md). From a clean
committed checkout, in PowerShell:

```powershell
npm ci
npm run test:release:windows
npm run test:packaging:windows
npm run build:windows
npm run bundle:windows
.\scripts\windows-artifact.ps1
.\scripts\windows-installer-artifact.ps1
```

The pinned Tauri bundler patches bundle-type bytes **while building the installer,
then restores the standalone executable**. Therefore the standalone hash is not
the installed payload hash, even after bundling. Installer metadata uses
`scripts/windows-installer-payload.ps1` to non-executingly extract the single
`new-app.exe` from the actual NSIS archive, verify its x64 GUI PE/identity/version
and unsigned state, and hash those complete unmodified bytes. The helper uses an
existing `7z.exe` on PATH or Chocolatey's bundled `tools\7z.exe`; it never installs
a dependency. Missing/ambiguous extraction fails rather than substituting a hash.

`application_sha256` identifies the bundled payload;
`standalone_application_sha256` separately identifies the restored build output.
The disposable upgrade fixture uses the same extraction/verification path.
Installed acceptance still requires exact full-file SHA-256 and both version
checks, recording expected/observed values in failure-retained diagnostics.
It does not normalize/ignore the bundle marker or accept a version-only match.
See the [pinned bundler patch/restore contract](https://github.com/tauri-apps/tauri/blob/8909f221d1515955fc843808032bdc5d62209c96/crates/tauri-bundler/src/bundle.rs).

Tauri uses `src-tauri\target\.tauri` for its
checksum-verified vendor tools; `CARGO_TARGET_DIR` keeps worker build outputs
separate. The unsigned installer is
`src-tauri\target\release\bundle\nsis\PR Sniper_VERSION_x64-setup.exe`.

The staging scripts refuse dirty source, mismatched CI commit, inconsistent
versions, a changed application, wrong PE architecture/subsystem, unexpected
installer identity/version or signed output in this unsigned lane. The installer
artifact contains only `pr-sniper-VERSION-x64-setup.exe`, `installer.json` and
`SHA256SUMS`. Metadata records the full commit, stable version, x64 target,
installer and application SHA-256 hashes and explicit unsigned/non-release
status. Existing artifact destinations are not overwritten.

This is a reproducible **build procedure with pinned inputs and exact-byte
provenance**, not a claim that independently linked or signed PE files are
bit-for-bit identical. An unsigned candidate using today's checked-in version
is not a new release or an addition to an existing public version.

## Chocolatey package source and pack-only proof

`scripts/windows-chocolatey.ps1` generates a nuspec, immutable metadata and the
two minimal package scripts from `packaging/chocolatey`. There are no package
dependencies and no embedded application binary in the public variant.
It uses Chocolatey's download/process helpers; `-Elevated:$false` avoids requesting
administrator rights. A globally installed Chocolatey CLI may itself require an
elevated shell for its own package database. The app still belongs to the
**invoking account**, never a guessed desktop user; do not install as SYSTEM or
another account expecting this user's tray app.

To produce and **pack, but not install**, a local test candidate:

```powershell
$directory = 'src-tauri\target\windows-installer-artifact'
$metadata = Get-Content "$directory\installer.json" -Raw | ConvertFrom-Json
.\scripts\windows-chocolatey.ps1 -Mode LocalTest `
  -Installer "$directory\$($metadata.filename)" -Version $metadata.version `
  -Sha256 $metadata.sha256 -Commit $metadata.commit `
  -Destination src-tauri\target\chocolatey-localtest
choco pack src-tauri\target\chocolatey-localtest\pr-sniper-localtest.nuspec `
  --output-directory=src-tauri\target\chocolatey-localtest
```

The test ID is **`pr-sniper-localtest`**, version `VERSION-localtest`. It embeds
the exact checksum-pinned unsigned installer, says **NEVER PUBLISH**, and requires
the hosted acceptance environment or an explicit operator opt-in for the exact
installer hash, described below. This is an accident guard, not a security
boundary against someone modifying scripts or environment variables.
There is no push/publishing command or
workflow for these packages; **never upload the test nupkg to any public feed**.
Generated installer executables have `.ignore` markers so Chocolatey does not
make command shims.

### Explicitly authorized local lifecycle testing

Local install/upgrade/uninstall requires separate human permission for this
application and a single owner coordinating its stop, preservation and restore.
After verifying the installer/package and preparing exact ownership receipts and
rollback, that owner may set the **process-only**
`PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256` to the independently verified SHA-256 of
the specific unsigned installer. A missing/different hash still refuses local
installation. Set the new exact hash separately for a test upgrade, then remove
the variable afterward. This affects only `LocalTest` packages; public trust,
checksum and release gates are unchanged. Do not spoof hosted-CI environment
flags or run the fresh-runner acceptance script on a used desktop.

A manually copied app is not an NSIS/Chocolatey installation and cannot be
"uninstalled" through those tools. Before migration, the lifecycle owner must
record its exact path/PID/hash and only its owned files/entries, arrange graceful
Quit, and preserve those files in a verified rollback location. Preserve the
application data directory and all credentials; do not export or enumerate
credentials, recursively sweep registry trees, or delete another installation's
entries. The installer intentionally refuses to overwrite an unowned executable:
move only the receipted manual application files after coordination, never the
entire application-data folder. Test through the private local feed, then restore
a working normal application and verify its identity/runtime. Do not leave a
disposable test-version application as the normal installation. Report each
actual operation rather than treating this authorization as acceptance evidence.

The public ID remains **`pr-sniper`**. An official-feed lookup on 2026-09-30
returned no versions; that is not a reservation, ownership grant or promise of
name availability. Authorship is Dylan McCurry; the project is
`https://github.com/jdylanmc/pr-sniper`. No repository-wide license was declared
at implementation time (GitHub reported `license: null`). Licenses inside agent
tooling are **not** the application's license. The test package intentionally
does not invent an OSS license. A human must approve actual distribution terms
and their HTTPS license URL before public generation/moderation.

`-Mode Public` requires that approved `-LicenseUrl`, `-ExpectedThumbprint`,
`-ExpectedSubject` and this exact versioned URL:
`https://github.com/jdylanmc/pr-sniper/releases/download/vVERSION/pr-sniper-VERSION-x64-setup.exe`.
It rejects unsigned/untrusted/untimestamped or wrong-publisher bytes before
emitting a package; independently checks SignTool policy; invokes the exact-tag
release gate below; and downloads the public URL without credentials to recheck
its SHA-256 and signature. It never treats a CI artifact URL or `latest` URL as
an immutable release. The installing client rechecks SHA-256 even if Chocolatey's
checksum feature was disabled, plus the expected Authenticode identity and
timestamp using inbox PowerShell/Windows trust. No Windows SDK installation is
required on client machines.

Uninstall uses an install receipt with the exact uninstaller hash and version,
and compares the exact current-user installer registration. NSIS's final
`_?=DIRECTORY` argument keeps uninstall synchronous; the package removes the
now-closed uninstaller only if its receipt hash still matches. It does not execute
an arbitrary registry command or recursively delete a directory.

### Community-validation checklist (#63)

Checked against the [official moderation requirements/guidelines](https://docs.chocolatey.org/en-us/community-repository/moderation/)
and [validator guidance](https://docs.chocolatey.org/en-us/community-repository/moderation/package-validator/)
on 2026-09-30; delivery status and native evidence were reconciled on 2026-10-01.
**Met** below means the stated local evidence at the recorded delivery, not
community approval or acceptance of later commits. **Deferred** needs owner
decisions or public-distribution validation; **N/A** is limited to the stated
variant. The service/moderator may enforce newer rules.

| Check                                          | Official expectation / scope                                                                                                                                 | Status   | Evidence or remaining action                                                                                                                                                                                                                                                                              |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Basic metadata                                 | Required project URL, appropriate author/title/description and non-abusive tags                                                                              | Met      | Generator names Dylan McCurry and the actual project, describes the per-user app/prerequisites, and avoids misleading tags.                                                                                                                                                                               |
| New package identity                           | Naming conventions apply to new submissions                                                                                                                  | Deferred | `pr-sniper` remains the proposed public ID; the empty feed lookup is not ownership or reservation. Maintainer/account confirmation remains external.                                                                                                                                                      |
| License, attribution and acceptance            | Include an available license URL; use accurate copyright and applicable trial/license-required disclosures                                                   | Deferred | Owner must approve actual terms, attribution and public metadata. The generator's current `requireLicenseAcceptance=false` is not approval of future terms. Do not invent a license/copyright or add trial/license tags without facts.                                                                    |
| Download contract                              | Project-origin download and package/installer version agreement; pinned checksum is this package's integrity contract                                        | Met      | Exact versioned project URL, SHA-256, version and public signature gates are implemented. This does not establish published signed bytes.                                                                                                                                                                 |
| WebView2 / dependencies                        | Verifier checks that the package installs correctly with appropriate runtime dependencies; the cited guidance does not mandate a particular WebView2 package | Deferred | Local design intentionally declares and checks **pre-installed WebView2 Evergreen**, rather than installing a Chocolatey dependency/bootstrapper. Confirm clean-verifier compatibility before public submission; any dependency/provisioning change needs a reviewed owner choice, not a guessed package. |
| Lifecycle / script behavior                    | Silent install and correct package install/uninstall; use appropriate Chocolatey helpers                                                                     | Met      | Private-feed pack/install/upgrade/uninstall, rollback/retry, preservation and clean reinstall passed in native validation. Interactive tray proof was verified separately on the authorized VM; see [delivery status](#delivery-status).                                                                  |
| Public embedded-binary redistribution evidence | Required when distributing embedded binaries                                                                                                                 | N/A      | Public variant downloads the installer; it does not embed it. The embedded unsigned local-test package must never be submitted. Application distribution rights remain an owner prerequisite.                                                                                                             |
| Additional architecture URLs                   | Include supported x86/x64 downloads when available                                                                                                           | N/A      | Only x64 is built for this delivery; no x86/ARM URL or architecture-dependent installer is invented.                                                                                                                                                                                                      |
| Icon and release notes                         | `iconUrl` and useful release notes are recommendations, not blanket required fields; a supplied icon must meet hosting/format rules                          | Deferred | Select an owner-controlled compliant icon URL and real release notes when public metadata is approved; do not add arbitrary placeholders or raw GitHub icon links.                                                                                                                                        |
| Public verification / moderation               | Validator/verifier checks and moderator approval are distinct from pack success                                                                              | Deferred | Trusted signing is additionally required by PR Sniper's release policy. Publisher identity/key, immutable public artifacts, submission and public-feed install/upgrade are not established.                                                                                                               |

Public partial-asset/submission recovery remains in
[trusted signing and publication](#trusted-signing-and-publication-deferred):
reconcile actual remote state, retain immutable bytes/tags and existing macOS
assets, and distinguish pending/rejected submissions from public approval.
The local-completion scope of #63 is closed; this checklist does not authorize
publication or satisfy the remaining public-distribution criteria in #62/#64.

## Hosted-only acceptance and evidence

For native refusal diagnosis, hosted acceptance sets the process-only
`PR_SNIPER_INSTALLER_DIAGNOSTICS` to a newly owned directory. The installer
exclusively creates one UTF-16 trace per process there, recording fixed lifecycle
stages and the original refusal/rollback text. It never replaces an existing log,
captures credentials/environment dumps, or writes diagnostics into app data.
Diagnostics do not change refusal codes or bypass ownership/runtime checks.
An explicitly requested but unusable diagnostic destination fails before mutation.

The separate `pr-sniper-windows-installer-diagnostics-COMMIT` artifact uploads
even when acceptance fails; it contains only these traces and source/hash/failure
context, plus the bounded test commands' raw `choco-*.log` output. These are
per-command fixture logs, not Chocolatey's global log. The failure path also
prints its own native traces into the job log,
without replacing the original failure if collection fails. No complete Chocolatey
log, application profile, credential data or whole target directory is uploaded.
Missing traces are not success: the process may have failed before trace setup.
The ordinary acceptance receipt remains success-only. A generic NSIS exit 2
(`Setup was cancelled` in Chocolatey) alone does not identify which gate failed.

When enabled, the Windows workflow runs **all** full native, offline runtime,
browser, formatting and frontend checks. It adds the unsigned installer artifact
after those checks, then builds an explicitly named **test-only** next-patch
version with a disposable Tauri JSON override. No checked-in release version or
tag changes; that artifact must never be released. Its PE version and hashes
are independently checked. No installer executes in the build job.

A separate `windows-installer-acceptance` job needs that successful native job
and runs on a fresh **GitHub-hosted Windows 2022 VM**, never self-hosted. Before
any installation, its script requires the hosted environment and absence of a
running host, application directory, profile, installer keys, startup values
and notification/Start Menu shortcuts. A different install path alone is not
isolation: these registry and shortcut identities are shared per user.

The job packs both local-test packages, installs from its private feed, verifies
actual installed bytes/versions, starts the real installed executable with a
unique profile/credential namespace, upgrades, verifies again and uninstalls.
It tests data and one exact synthetic Credential Manager record surviving both
operations, plus unrelated Run values, an unrelated file/registry value and a
retargeted shortcut surviving removal. It never signs in or enables automation.
Only the exact PID it launched is stopped; failure cleanup preserves the original
failure status. The disposable VM is then retired by GitHub.

`acceptance.json` is uploaded only after success. It distinguishes a live host
with a fresh owned profile's notification identity and `session_started` record
from **actual interactive tray/menu acceptance**. A nonzero `MainWindowHandle`
may belong to Tauri's internal single-instance window and is not rejected.
That smoke cannot prove tray icon visibility, menu actions, real logon or
notification delivery. The real later-release upgrade and public-feed install
remain separate. Each new candidate needs its own green hosted run; build/helper
checks or an older run do not establish CI acceptance. The
[recorded delivery](#delivery-status) has both successful
hosted lifecycle acceptance and separate local interactive proof.

Hosted fault fixtures lock only the owned uninstaller/shortcut, deny writes on
only the owned installer key and exercise test-only injected failures after file
and registry transitions. They independently compare old hashes, typed registry
values and the retained Chocolatey receipt, then release the obstruction and
retry. Injection is compiled only into the disposable upgrade fixture, never the
ordinary installer. Its WebView2 lookups use a unique synthetic key (machine
miss/current-user hit); no real runtime keys, installation or policy are changed.
For both SetValue and Delete denial, the fixture first retains a handle to the
exact current-user, 64-bit installer key with only `ReadPermissions` and
`ChangePermissions`. It snapshots the original DACL, applies and verifies the
single intended denial, then restores and verifies the original DACL
through that same handle. This avoids `Set-Acl` reopening the path with ordinary
value-write rights that the fixture deliberately denied. No owner, group, SACL
or parent-key permissions are changed. The product still uses its own separately
opened handles; independent state snapshots are not taken through the restore
handle. A primary assertion failure remains primary if restoration also fails;
the additional cleanup failure is reported, never interpreted as acceptance.
Readback admits only the demonstrated kernel **addition** of
`SE_DACL_AUTO_INHERITED` bookkeeping among DACL control flags. The helper extracts
the `RawSecurityDescriptor.DiscretionaryAcl` and compares its binary form exactly,
including revision and every ACE's type/order, SID, mask and inheritance flags.
DACL presence, defaulted/untrusted/server-security flags, protection and inheritance
requirements must match; removal of the bookkeeping bit still fails. Invalid,
missing or null DACLs are refused. Optional owner/group metadata and offsets in
the surrounding Access-only descriptor are not DACL authorization state; their
representation may change without changing the ACL. No owner/group writes are
performed, and real permission drift is not normalized.
They also deny shortcut reads for direct/package removal, deny only registry
Delete, and exercise a read-only post-native uninstaller plus a deterministic
other-thread cleanup mutex conflict before retrying the same package. Unknown
values, subkeys and a readable retargeted shortcut must survive.

Never run either installer or `choco install/upgrade/uninstall` as incidental
validation on a shared machine with an existing app. Pack-only tests do not
prove installed behavior. Source/helper tests never execute their synthetic PE
fixture or touch native application registrations/credentials.

## Trusted signing and publication: deferred

Public Windows distribution is deferred, not an unfinished local acceptance
step. No provider/account was selected or provisioned by the Windows delivery.
Retain the requirement for
**publicly trusted Windows Authenticode signing and a trusted timestamp**.
There is no self-signed/ad-hoc fallback, borrowed unrelated certificate, trust-root
installation or credential upload in this delivery.

The human/external operator must:

1. Select and own a publicly trusted code-signing identity/service for PR Sniper;
   complete its identity validation and securely scoped CI access. Establish the
   independently verified expected certificate thumbprint and exact subject,
   timestamp service, rotation/expiry handling and any provider-specific signer.
   This document does not choose a paid provider or claim an account exists.
2. Add a **separate protected tag-only signing/publication route**, not secrets
   to ordinary PR CI. Sign the application **before bundling**, configure Tauri's
   real `bundle.windows.signCommand` for the chosen signer (including the generated
   uninstaller), then sign the final installer. The ordinary `--no-sign` command
   is never the release signing path. Do not insert a guessed command/credential.
3. Verify all actual signed outputs with `Assert-TrustedWindowsSignature`
   (`scripts/windows-signature.ps1`), the expected subject/thumbprint and
   `-RequireSignTool`; check version/architecture and validate the extracted
   installed app/uninstaller on the fresh runner as well. The helper requires
   Windows `Valid` embedded Authenticode, a non-self-signed code-signing leaf,
   a timestamp certificate and successful `signtool verify /pa /all /tw`.
   Tests of this helper do not prove PR Sniper has been signed.
4. Human chooses a real stable version/tag at the exact green main commit.
   `python -B scripts/release.py check-windows-release` is a **read-only** future
   preflight: use the same tag/repository/read-token environment as the macOS
   gate. It adds the latest exact-commit **main-push** Windows run and both native
   and installer-acceptance jobs to the existing macOS/version/ancestry/tag
   checks. Pending, skipped, PR-only or other-commit checks never qualify.
5. Implement immutable Windows asset publication without replacing macOS's
   existing ZIP/manifest/checksum assets or invoking/rewriting the tap publisher.
   Reconcile any draft/partial/uncertain publication; never move an existing tag
   or replace signed bytes. The existing macOS release workflow and its exact
   asset contract are unchanged; it does **not** currently publish Windows.
6. Confirm Chocolatey maintainer identity, `pr-sniper` name ownership, approved
   license/distribution metadata and scoped publisher API key. Independently
   review and explicitly authorize the first submission. Record **submitted**,
   **awaiting moderation**, **rejected** and **publicly approved** separately.
   A successful push, local pack or local-feed install is not public availability.
   Respond to moderation requirements, reconcile failed submissions, and verify
   public-feed installation plus a subsequent approved release upgrade.

The completed private-package scope of #63 does not complete #62 or #64.
Neither unsigned candidate acceptance nor the CI pause authorizes a signed
Windows release or public Chocolatey submission.

## Primary contracts checked

- [Tauri Windows installer/configuration](https://v2.tauri.app/distribute/windows-installer/)
- [Pinned Tauri NSIS template and utilities](https://github.com/tauri-apps/tauri/tree/8909f221d1515955fc843808032bdc5d62209c96/crates/tauri-bundler/src/bundle/windows/nsis)
- [Pinned process plugin: FindProcessCurrentUser returns 1 for absent, not KillProcess's 2](https://github.com/tauri-apps/nsis-tauri-utils/blob/13d9edd27b69310e108d6fbd49f90992f8a05390/crates/nsis-process/src/lib.rs)
- [Chocolatey pack CLI](https://docs.chocolatey.org/en-us/create/commands/pack/)
- [Download/checksum helper](https://docs.chocolatey.org/en-us/create/functions/get-chocolateywebfile/)
- [Process helper and explicit non-elevation](https://docs.chocolatey.org/en-us/create/functions/start-chocolateyprocessasadmin/)
- [Chocolatey 2.7.4 post-install file snapshot](https://github.com/chocolatey/choco/blob/2.7.4/src/chocolatey/infrastructure.app/services/ChocolateyPackageService.cs#L507-L510)
- [Chocolatey 2.7.4 tracked-path/checksum uninstall rules](https://github.com/chocolatey/choco/blob/2.7.4/src/chocolatey/infrastructure.app/services/NugetService.cs#L3024-L3121)
- [Community moderation/metadata requirements](https://docs.chocolatey.org/en-us/community-repository/moderation/)
- [Official package-name lookup](<https://community.chocolatey.org/api/v2/FindPackagesById()?id=%27pr-sniper%27>)
