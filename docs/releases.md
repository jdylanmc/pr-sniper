# macOS releases and Homebrew

This is the human-approved macOS distribution slice of
[#12](https://github.com/jdylanmc/pr-sniper/issues/12), separate from the earlier
local-app foundation. Initial releases support **Apple Silicon and macOS 13.5+**.
Windows, Intel, App Store distribution and #14's in-app/dogfood updater are not
part of this delivery.

The later authorized Windows follow-up has an
[unsigned installer/Chocolatey candidate lane](windows-packaging.md).
It does not change this macOS workflow, published asset set or tap ownership.
Windows public signing, publication and Chocolatey moderation are still blocked
on explicit external prerequisites; an unsigned CI artifact is not a release.

**Published:** [v0.1.1](https://github.com/jdylanmc/pr-sniper/releases/tag/v0.1.1)
passed real Developer ID signing, notarization/stapling and public-byte
verification. Its cask is on the dedicated tap's main branch. A disposable hosted
CI installation passed version/signature/ticket/Gatekeeper verification and
uninstallation. Full signed-in app operation and a later-version upgrade remain
separate acceptance; no developer installation was overwritten.

## What a version tag does

Pushing a stable `vMAJOR.MINOR.PATCH` tag automatically releases and updates
`jdylanmc/homebrew-pr-sniper` without an additional human approval prompt.
Repository writers who can change workflows or create matching release tags
are trusted release operators. Protect that access; credentials are not safe
from a malicious trusted workflow writer merely because this workflow has a
gate.

The pipeline:

1. Require the tag to match all checked-in version fields, resolve to the exact
   checkout and be on `main`. The latest **main-push** macOS CI for that exact
   commit must have completed successfully. PR checks for another SHA do not
   qualify. No release while required CI is pending or failed.
2. Before any Apple upload, confirm the dedicated tap token can write the exact
   public tap and that its publisher workflow is merged. Then build on the
   pinned Apple Silicon macOS runner using the normal credential-free
   build. Normal PR CI stays ad-hoc signed and never receives Apple/tap secrets.
3. In an isolated temporary keychain, import the dedicated certificate and
   require its exact SHA-1 fingerprint, Developer ID Application type and team.
   Prepend that owned keychain to the existing user search list and verify
   readback before resolving/signing. Restore and verify the original list
   before deleting the temporary keychain; do not change the default keychain.
   Sign the app with hardened runtime and a secure timestamp; verify identity,
   version, architecture, signature chain and absence of unexpected entitlements.
   No `--deep` signing, keychain default changes or ad-hoc fallback.
4. Submit to Apple's notary service; require **Accepted**, staple the app,
   validate its ticket and Gatekeeper assessment, then build the final ZIP.
   Unpack and recheck that exact archive. Restore keychain search state and
   remove temporary credentials before success.
5. Create a draft release at the immutable tag and commit. Upload only the final
   ZIP, `manifest.json` and `SHA256SUMS`; verify GitHub's asset digests before
   publishing, then download the public assets and verify every byte.
6. Only after public verification, dispatch the version once to the dedicated
   tap's main-branch publisher. The tap independently verifies the public
   release, runs direct Homebrew online audits and an isolated CI install,
   checks the installed version/signature/ticket/Gatekeeper, then uninstalls
   the CI-owned app. Only afterward does it update `Casks/pr-sniper.rb` using
   its own repository-scoped token. Compare-and-swap file identity and version
   checks prevent concurrent overwrite, downgrade or same-version replacement.
   The application job waits for exact matching cask bytes; a timeout is
   unconfirmed, not success or an automatic redispatch.

The existing `com.jdylanmc.pr-sniper` app identity is unchanged. Homebrew manages
updates; there is no in-app updater and no `auto_updates` cask claim.
The cask uses Homebrew's current Ventura-or-later OS-family declaration.
The app's verified `LSMinimumSystemVersion` and the cask caveat specify the
precise **13.5** minimum; Homebrew's OS-family declaration alone is not a
minor-version runtime check.

## One-time Apple/GitHub setup

Use an active paid personal Apple Developer Program membership. Credentials
created for PR Sniper are independent of Notch's working credentials; never
revoke Notch's certificate/key to make room without a separate decision.

1. In Keychain Access, generate a new Certificate Signing Request. Through
   Apple Developer **Certificates**, issue a **Developer ID Application**
   certificate and install it with its corresponding private key. Export only
   that identity as password-protected `.p12`.
2. In App Store Connect **Users and Access > Integrations > Team Keys**, create
   a `PR Sniper Notarization` key with Developer access. Download its `.p8` once
   and save the Key ID and Issuer ID. Apple team keys are not app-restricted;
   a dedicated name enables independent rotation, not narrower Apple permissions.
3. Create GitHub environments below with **selected tag policies `v*` only**,
   not all branches, and no approval requirement for the requested automatic
   release mode. The workflow additionally enforces strict stable tag syntax.
4. Upload secret values directly through GitHub/`gh secret set`; never paste
   them into chat, source, workflow files or command-line `--body` arguments.
   A base64 P12 remains secret; base64 is not encryption.

| Environment         | Secret                       | Format                                                                                                       |
| ------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `pr-sniper-release` | `APPLE_CERTIFICATE_P12`      | Base64 of the exported `.p12`                                                                                |
| `pr-sniper-release` | `APPLE_CERTIFICATE_PASSWORD` | Its export password, not the Apple account password                                                          |
| `pr-sniper-release` | `APPLE_NOTARY_KEY_P8`        | Raw downloaded PEM private-key text                                                                          |
| `pr-sniper-tap`     | `HOMEBREW_TAP_TOKEN`         | Fine-grained token scoped only to `jdylanmc/homebrew-pr-sniper`, Contents read/write for repository dispatch |

The release environment also has **variables** (public metadata):
`APPLE_TEAM_ID`, `APPLE_SIGNING_IDENTITY` (the exact uppercase 40-hex certificate
SHA-1 fingerprint, not its potentially ambiguous name), `APPLE_NOTARY_KEY_ID`
and `APPLE_NOTARY_ISSUER_ID`. No personal values are hardcoded in the repository.

The P12 password and native keychain password are passed to Apple's native
keychain CLI as required by that tool. Commands run without shell tracing;
credential subprocess output and argv are not echoed. The temporary keychain
password is randomly generated, not a persisted GitHub secret. Only final public
assets enter the artifact upload directory.

Record certificate/key/token expiration privately. Rotate by updating these
environment entries and verifying the next authorized release; do not mutate an
existing release or remove a previous working certificate first.

The tap repository stores only the public expected `APPLE_TEAM_ID` variable.
Its `Publish verified cask` workflow uses the standard scoped GitHub token for
its own cask commit; it receives no Apple secrets. Its publisher workflow must
be merged on tap `main` before the application's release preflight succeeds.

## Cut a release

The human chooses the version and creates the tag. This delivery does not
authorize an agent to cut the first release merely because checks pass.

1. Update `package.json`, both root npm lock version fields,
   `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and the PR Sniper entry in
   `src-tauri/Cargo.lock` to one stable version. Curate `CHANGELOG.md`.
2. Merge the version/release changes to main. Wait for that merged commit's
   normal **macOS foundation** workflow to pass, including `npm run test:release`.
3. Create and push the matching annotated `vMAJOR.MINOR.PATCH` tag at the exact
   verified commit. Never move or overwrite a published version tag.
4. Observe **Signed macOS release**. Missing credentials, Apple denial,
   identity mismatch, invalid notarization and failed byte verification stop
   publication. A tap failure does not roll back an already published valid
   release; recover using the tap repository's `Publish verified cask` workflow
   on `main`, selecting the already-published tag. Do not rerun signing.

Workflow dispatch is a recovery entry point: run it **on an existing version
tag**, not `main`. The same tag/commit/CI/environment checks still apply.
Release execution is serialized, without cancelling an active signing run.

## Public artifacts and recovery

- `pr-sniper-VERSION-aarch64-apple-darwin.zip`: stapled Developer ID-signed app.
- `SHA256SUMS`: SHA-256 of the final ZIP, not the pre-stapling submission.
- `manifest.json`: exact version/tag/commit, file/checksum, arm64 architecture,
  app/team identity and Apple's accepted submission ID. No credentials.

An identical published release/cask can be verified without another mutation.
A regenerated signature/archive will not necessarily have identical bytes:
do not re-sign and overwrite an existing version. Preserve the successful
signed artifact and rerun only failed downstream jobs while the workflow
artifact is retained. Public release assets remain the durable distribution.

An uncertain upload/create may leave a draft. The workflow refuses to delete,
replace or automatically publish that partial draft. Inspect its exact tag,
commit and asset hashes, reconcile deliberately or choose a new version.
Published-byte mismatch or tag movement stops tap publication. A failed cask
audit leaves the valid GitHub release available but the prior tap unchanged.

Forced runner loss can interrupt cleanup; GitHub-hosted disposable runners are
required. This workflow is not designed for a shared persistent self-hosted
signing machine. No release step modifies developer app data, login items,
notification settings or GitHub/Copilot credentials.

Native stage logs contain fixed operation labels, exit codes and allowlisted
error categories, not raw stderr, command arguments or credentials. For example,
`codesign/sign` is distinct from `codesign/verify` and `codesign/entitlements`;
`security-internal-component` or `certificate-chain` remains a diagnostic hint,
not permission to skip a signature check. Empty required configuration reports
the missing names only.

The initial `v0.1.0` attempts failed without publishing assets: first an empty
certificate secret, then an opaque native `codesign` failure. The certificate
upload was corrected. The `0.1.1` recovery adds the missing keychain search-list
setup and bounded diagnostics; it does not prove live signing until its own
authorized release runs. Keep `v0.1.0` unchanged rather than moving the tag.

## First-release acceptance

Workflow unit/contract tests and an ad-hoc build do **not** prove Developer ID
signing, Apple notarization or a working Homebrew installation.

After explicit first-tag authorization and successful release:

```sh
brew install --cask jdylanmc/pr-sniper/pr-sniper
```

Verify the downloaded version/checksum, Developer ID/team signature, stapled
ticket and Gatekeeper acceptance without disabling quarantine or security
checks. Test launch, menu bar, quit, existing state preservation, and a later
real version upgrade using `brew update` / `brew upgrade --cask pr-sniper`.
Never overwrite a developer's installed app as an incidental test: use a
separate installation directory or obtain explicit install/upgrade permission.
Keep update #14 open; standard Homebrew upgrade support is not an automatic
background application updater.
