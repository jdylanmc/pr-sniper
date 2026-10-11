#!/bin/bash
# Dev-only cargo runner: sign the freshly built binary with a stable identity
# before launching it, so macOS keychain "Always Allow" survives rebuilds.
# Ad-hoc dev binaries get a new code identity on every build and re-prompt.
#
# Usage (wired by `npm run dev:signed`):
#   CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER=$PWD/scripts/dev-sign-run.sh tauri dev
# Override the identity with PR_SNIPER_DEV_SIGN_IDENTITY="Apple Development: ...".
set -euo pipefail
bin="$1"
shift

identity="${PR_SNIPER_DEV_SIGN_IDENTITY:-}"
if [ -z "$identity" ]; then
  identity="$(security find-identity -v -p codesigning |
    sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' | head -1)"
fi

if [ -n "$identity" ]; then
  codesign --force --sign "$identity" \
    --identifier com.jdylanmc.pr-sniper.dev "$bin" >/dev/null 2>&1 ||
    echo "dev-sign: signing failed; running with the ad-hoc identity" >&2
else
  echo "dev-sign: no Apple Development identity found; running ad-hoc" >&2
fi
exec "$bin" "$@"
