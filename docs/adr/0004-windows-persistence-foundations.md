# Keep Windows persistence private without pretending the native port is complete

## Scope

The authorized existing-app Windows follow-up (#57) extends ADR-0001's
filesystem boundary, not the product or provider-account model. Credentials
still do not belong in configuration, state or diagnostics. Native Windows
credentials/Copilot execution (#58), tray/login/bundle identity (#59) and
notifications (#60) remain separate adapters. No unsupported-success adapter
is introduced.

## Storage boundary

The Windows default data root uses Tauri's current-user local application data
directory (`%LOCALAPPDATA%\com.jdylanmc.pr-sniper`). macOS retains its existing
application data directory. An explicit `PR_SNIPER_DATA_DIR` still requires an
absolute path and preserves the existing isolated-profile/login-mutation
boundary. Full isolated native-account launch also needs #58's credential
namespace implementation; a filesystem-only test is not a native app launch.

Reads validate owned root/config/state directories and files, rejecting
symlinks/reparse points and returning actual access errors. They never chmod,
replace access-control lists, or create missing state directories. An unreadable
saved configuration remains an error, not a repaired or empty profile. First
settings load still initializes a genuinely missing configuration through the
separate private write path. The caller must choose a trusted current-user root,
not a shared directory.

On macOS/Unix, new directories use `0700` and new files `0600`; existing
permissions are preserved. On Windows, the
native security APIs create directories with a protected discretionary access
control list (DACL), granting only the current process user full control.
Directory grants are inheritable to new files/subdirectories; owned files are
then protected explicitly before writing. Windows write operations establish
private owned paths; reads do not reset existing permissions or require permission
administration rights. Existing ancestor directories outside the owned root are
not repermissioned. Windows readonly attributes are **not** a privacy boundary.
Failure to read the user identity or establish the DACL fails the operation
visibly; there is no permissive fallback for a filesystem without ACL support.
This is user isolation, not encryption or protection against the same user,
administrators exercising privileged recovery, or a compromised host.

Settings and all typed state saves share the same real atomic writer:
exclusively create a sibling `.tmp`, establish permissions, write/flush, close
the handle, then rename over the destination. Closing first supports Windows
replacement semantics. A failed replacement preserves the previous file and
removes only the stage this operation created. An occupied stage is neither
truncated nor removed: after a crash, inspect it and remove it explicitly before
retrying. A cleanup failure is distinct and visible. This guarantees atomic
replacement, not transactional multi-file commits or power-loss durability of
the parent directory.

Validation, stale-draft detection, strict schemas, doctrine initialization and
the committed-save/separate-diagnostic-warning contract remain unchanged.
Diagnostic rotation and append use the same private filesystem boundary.

## Build and evidence boundary

`plist` and Objective-C dependencies are macOS-target dependencies.
LaunchAgent and notification implementations are macOS-only modules.
Unadapted callers still fail to compile on Windows rather than receiving a
fake implementation. Executable lookup retains absolute PATH entries and
macOS Homebrew locations, adds Windows `.exe` lookup, and does not add shell,
current-directory or ambient-credential fallback.

`src-tauri/foundations` is a small, non-published test harness in the existing
Cargo workspace. It imports **the exact production** storage, private
filesystem, discovery, policy, doctrine seeding and executable-lookup source
files and runs the existing relevant integration tests plus native filesystem
failures.
Only the typed queue/review/publication/notification state methods move to a
separate `Store` implementation; their common reader/writer stays in production
storage and is exercised by real saved queue-selection state. No replacement
domain types or duplicated storage implementation are used.

There is one lockfile. The app remains the default workspace member, so existing
macOS application checks/tests are not replaced by the harness. Windows CI
runs native source formatting, the harness's native tests and strict Clippy
checks in addition to the existing frontend/shared checks. This does not build,
package or launch the Windows app. See [Windows development](../windows-development.md) for commands,
remaining blockers and convergence acceptance.
