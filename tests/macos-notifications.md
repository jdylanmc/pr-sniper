# Native notification acceptance

This covers the approved macOS P11 slice of #15. Browser/native-Store tests do
not prove OS banner presentation or the notification-center callback.

## Isolation

- Confirm the desktop is unlocked and Accessibility/Screen Recording permission
  is available. A locked desktop is **unverified**, not a passed visual check.
  Do not unlock it or change global Focus settings on the user's behalf.
- Build a test-specific bundle identifier/product name through Tauri's existing
  `--config` option. For example, use a unique
  `com.jdylanmc.pr-sniper.tests.notifications-<id>` identifier and
  `PR Sniper Notifications Test` product name. macOS authorization belongs to a
  bundle identifier, not an app-data profile; a separate test bundle prevents
  changing production PR Sniper notification permission.
- Use the configured ad-hoc bundle signing, then verify with
  `codesign --verify --strict` and `codesign -dv --verbose=2`. The signature's
  identifier must match Info.plist and bind it; an executable-only compiler
  signature was observed to fail native authorization.
- Always launch the bundle executable with an absolute test-owned
  `PR_SNIPER_DATA_DIR` and unique
  `PR_SNIPER_KEYCHAIN_SERVICE=com.jdylanmc.pr-sniper.tests.<id>`. Never omit these
  variables during test restarts or fall back to production credentials.
- Seed only synthetic local queue data through the existing `settings_bridge`
  fixture interface. Keep automatic review/publication off and do not sign into
  GitHub or Copilot. Disconnected synthetic accounts should remain honestly
  blocked; the test-notification path still supports their exact queue identity.
- Do not click an isolated-profile notification while its test process is
  stopped: an OS cold launch does not retain custom environment variables.
  Relaunch explicitly with the same isolation variables before restart checks.

## Observe the real boundary

1. Open Settings > Preferences. Notifications must be off initially. Opt in,
   complete the normal macOS permission prompt for the **test application**, and
   verify the displayed native authorization/banner/Notification Center status.
   Denial or timeout must remain visible and must not claim opt-in success.
2. Select a specific synthetic queue item as **Test destination**, recording its
   saved ID and the resulting notification request ID from the private ledger.
   **Send test notification**. Observe the real banner or Notification Center
   item: test-app identity and generic text, with no PR/repository title or code.
3. Open an unsaved Settings editor, hide the panel, then click that exact OS
   notification. Verify the same native panel opens the single detail layer
   for that provider/account/repository/PR/iteration identity, without a reload.
   Return to Settings and verify the draft is unchanged.
   `state/queue-selection.json` must contain that item ID, the matching notice
   must have `opened_at`, and fixed-schema diagnostics must contain
   `notification_activated` followed by `notification_opened`. Clicking the
   in-app history button alone is not native callback evidence.
4. Confirm navigation did not start a review or publish/approve/merge anything.
   An unrelated or missing-profile request must never open a substitute item.
   A valid saved notification whose exact destination is now unavailable must
   open the explicit missing-destination state, not a first/last queue item.
5. Wait through at least two scheduler ticks. Unchanged source state must not
   add requests. Restart the test process with the same profile and Keychain
   namespace, then repeat the count and exact-destination checks.
6. Verify Settings-destination tests as well. Inspect notification history:
   accepted is still **unconfirmed visibility**, not acknowledgment; denied,
   failed, unknown and not-sent states retain in-app context. Focus behavior
   remains under the user's existing OS settings; no critical/time-sensitive
   authorization is requested.

## Evidence and cleanup

Record source commit, test bundle identifier, test PID, request/item IDs, actual
UI observations and timestamps. Capture only the test app/notification region;
do not capture unrelated desktop or Notification Center contents. Never convert
OS request acceptance, an injected callback or a screenshot into proof of a
human's review or acknowledgment.

Dismiss only this test application's alerts, stop only the recorded test-owned
process, and remove only its explicitly identified temporary profile, helpers
and bundle. Restore the ordinary shipping bundle with `npm run bundle` after a
test-specific build. Retain redacted evidence outside source control. Record
unavailable native observations plainly; do not silently replace them with
fixture results.
