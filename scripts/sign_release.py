#!/usr/bin/env python3
"""Build a Developer ID release without changing the user's keychain or app data."""

import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import secrets
import signal
import shlex
import shutil
import subprocess
import sys
import tempfile

from release import ASSETS, BUNDLE_ID, REPOSITORY, ReleaseError, config_version, filename, require, version


def run(args, timeout=120):
    try:
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("APPLE_") and key not in ("GITHUB_TOKEN", "HOMEBREW_TAP_TOKEN")}
        result = subprocess.run(args, capture_output=True, check=False, timeout=timeout, env=environment)
    except (OSError, subprocess.TimeoutExpired):
        raise ReleaseError("Native release stage could not complete: " + str(args[0])) from None
    require(result.returncode == 0, "Native release stage failed: " + str(args[0]))
    return result.stdout or result.stderr


def configuration():
    values = {name: os.environ.get(name, "") for name in (
        "APPLE_CERTIFICATE_P12", "APPLE_CERTIFICATE_PASSWORD", "APPLE_NOTARY_KEY_P8",
        "APPLE_NOTARY_KEY_ID", "APPLE_NOTARY_ISSUER_ID", "APPLE_TEAM_ID", "APPLE_SIGNING_IDENTITY")}
    require(all(values.values()), "Missing Apple signing/notarization configuration; inspect required secret/variable names.")
    require(re.fullmatch(r"[A-Z0-9]{10}", values["APPLE_TEAM_ID"]), "Invalid Apple Team ID.")
    require(re.fullmatch(r"[A-F0-9]{40}", values["APPLE_SIGNING_IDENTITY"]), "Signing identity must be the exact certificate SHA-1 fingerprint.")
    require(re.fullmatch(r"[A-Z0-9]{10}", values["APPLE_NOTARY_KEY_ID"]), "Invalid notarization Key ID.")
    require(re.fullmatch(r"[a-f0-9-]{36}", values["APPLE_NOTARY_ISSUER_ID"]), "Invalid notarization Issuer ID.")
    require(values["APPLE_NOTARY_KEY_P8"].strip().startswith("-----BEGIN PRIVATE KEY-----")
            and values["APPLE_NOTARY_KEY_P8"].strip().endswith("-----END PRIVATE KEY-----"),
            "Notarization private key must be the downloaded PEM, not a pathname or base64 wrapper.")
    try:
        certificate = base64.b64decode("".join(values["APPLE_CERTIFICATE_P12"].split()), validate=True)
    except ValueError:
        raise ReleaseError("Signing certificate must be base64-encoded P12.") from None
    require(certificate, "Signing certificate is empty.")
    return values, certificate


def verify_app(app, release_version, team=None):
    require(app.is_dir() and not app.is_symlink(), "Expected built application bundle.")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    require(info.get("CFBundleIdentifier") == BUNDLE_ID
            and info.get("CFBundleShortVersionString") == release_version
            and info.get("CFBundleVersion") == release_version
            and info.get("CFBundleExecutable") == "pr-sniper"
            and info.get("LSMinimumSystemVersion") == "13.5",
            "Built application identity, version or minimum OS does not match the release.")
    binary = app / "Contents/MacOS/pr-sniper"
    require(binary.is_file() and not binary.is_symlink(), "App executable is unavailable.")
    require(run(["lipo", "-archs", str(binary)]).decode().strip() == "arm64", "Release must contain Apple Silicon code only.")
    # This release supports the present single-native-executable bundle. Adding
    # native helpers requires an explicit inside-out signing/verification plan.
    magic = {b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xcf",
             b"\xfe\xed\xfa\xce", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca",
             b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca"}
    for path in app.rglob("*"):
        require(not path.is_symlink(), "Unexpected symlink in the release app; inspect before distributing.")
        if path.is_file():
            with path.open("rb") as stream:
                header = stream.read(4)
            require(header not in magic or path == binary, "Unexpected nested native code requires explicit release signing support.")
    run(["codesign", "--verify", "--strict", str(app)])
    signature = run(["codesign", "-dv", "--verbose=4", str(app)]).decode()
    require(f"Identifier={BUNDLE_ID}\n" in signature and "Info.plist entries=" in signature,
            "Code signature does not bind the expected bundle identity.")
    if team is not None:
        require(f"TeamIdentifier={team}\n" in signature and "Authority=Developer ID Application:" in signature
                and "Authority=Developer ID Certification Authority" in signature
                and "Authority=Apple Root CA" in signature and "Timestamp=" in signature
                and "runtime" in signature and "Signature=adhoc" not in signature,
                "Distribution requires Developer ID, exact team, hardened runtime and secure timestamp.")
        entitlement_output = run(["codesign", "-d", "--entitlements", ":-", str(app)])
        start = entitlement_output.find(b"<?xml")
        if start >= 0:
            end = entitlement_output.find(b"</plist>", start)
            require(end >= 0, "Malformed signed entitlements.")
            require(plistlib.loads(entitlement_output[start:end + 8]) == {},
                    "Unexpected app entitlements; distribution cannot silently add exceptions.")


def sign():
    require(platform.system() == "Darwin" and platform.machine() == "arm64", "Signing requires an Apple Silicon macOS runner.")
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY
            and os.environ.get("GITHUB_REF_TYPE") == "tag"
            and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted",
            "Signing is restricted to this product's tag workflow on a disposable hosted runner.")
    release_version = version(os.environ.get("GITHUB_REF_NAME", ""))
    require(config_version() == release_version, "Release tag/version mismatch.")
    sha = run(["git", "rev-parse", "HEAD"]).decode().strip()
    require(sha == os.environ.get("SOURCE_SHA"), "Signing source is not the gated commit.")
    values, certificate = configuration()
    app = Path("src-tauri/target/release/bundle/macos/PR Sniper.app").resolve()
    verify_app(app, release_version)
    require(not ASSETS.exists(), "Release output already exists; do not overwrite earlier signed assets.")
    previous_keychains = shlex.split(run(["security", "list-keychains", "-d", "user"]).decode())
    created_keychain = False
    cleanup_error = None
    with tempfile.TemporaryDirectory(prefix="pr-sniper-sign-", dir=os.environ["RUNNER_TEMP"]) as temporary:
        private = Path(temporary)
        keychain = private / "release.keychain-db"
        password = secrets.token_urlsafe(48)
        p12 = private / "certificate.p12"
        p8 = private / ("AuthKey_" + values["APPLE_NOTARY_KEY_ID"] + ".p8")
        p12.write_bytes(certificate)
        p8.write_text(values["APPLE_NOTARY_KEY_P8"])
        p12.chmod(0o600)
        p8.chmod(0o600)
        try:
            run(["security", "create-keychain", "-p", password, str(keychain)])
            created_keychain = True
            run(["security", "set-keychain-settings", "-lut", "3600", str(keychain)])
            run(["security", "unlock-keychain", "-p", password, str(keychain)])
            run(["security", "import", str(p12), "-k", str(keychain), "-P", values["APPLE_CERTIFICATE_PASSWORD"],
                 "-T", "/usr/bin/codesign"])
            run(["security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s",
                 "-k", password, str(keychain)])
            identities = run(["security", "find-identity", "-v", "-p", "codesigning", str(keychain)]).decode()
            matching = re.findall(r'\b([A-F0-9]{40}) "([^"]+)"', identities)
            require(len(matching) == 1 and matching[0][0] == values["APPLE_SIGNING_IDENTITY"]
                    and matching[0][1].startswith("Developer ID Application:")
                    and matching[0][1].endswith("(" + values["APPLE_TEAM_ID"] + ")"),
                    "Imported identity is not the pinned PR Sniper Developer ID certificate/team.")
            run(["codesign", "--force", "--sign", values["APPLE_SIGNING_IDENTITY"],
                 "--keychain", str(keychain), "--options", "runtime", "--timestamp",
                 "--identifier", BUNDLE_ID, str(app)], timeout=300)
            verify_app(app, release_version, values["APPLE_TEAM_ID"])
            submission = private / "submission.zip"
            run(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(submission)], timeout=300)
            try:
                result = json.loads(run(["xcrun", "notarytool", "submit", str(submission),
                                         "--key", str(p8), "--key-id", values["APPLE_NOTARY_KEY_ID"],
                                         "--issuer", values["APPLE_NOTARY_ISSUER_ID"],
                                         "--wait", "--timeout", "30m", "--output-format", "json"],
                                        timeout=1900))
            except ValueError:
                raise ReleaseError("Apple did not return a valid notarization receipt.") from None
            receipt = result.get("id", "")
            require(re.fullmatch(r"[a-fA-F0-9-]{36}", receipt), "Apple notarization receipt is missing.")
            require(result.get("status") == "Accepted",
                    f"Apple notarization was not accepted. Submission {receipt}; inspect Apple's log before retrying.")
            run(["xcrun", "stapler", "staple", str(app)], timeout=300)
            run(["xcrun", "stapler", "validate", str(app)])
            verify_app(app, release_version, values["APPLE_TEAM_ID"])
            run(["spctl", "--assess", "--type", "execute", "--verbose=2", str(app)])
            final_zip = private / filename(release_version)
            run(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(final_zip)], timeout=300)
            unpacked = private / "unpacked"
            run(["ditto", "-x", "-k", str(final_zip), str(unpacked)], timeout=300)
            unpacked_app = unpacked / "PR Sniper.app"
            verify_app(unpacked_app, release_version, values["APPLE_TEAM_ID"])
            run(["xcrun", "stapler", "validate", str(unpacked_app)])
            run(["spctl", "--assess", "--type", "execute", str(unpacked_app)])
            checksum = hashlib.sha256(final_zip.read_bytes()).hexdigest()
            manifest = {"version": release_version, "tag": "v" + release_version, "source_sha": sha,
                        "filename": final_zip.name, "sha256": checksum, "architecture": "arm64",
                        "bundle_id": BUNDLE_ID, "team_id": values["APPLE_TEAM_ID"], "notarization_id": receipt}
            # Promote only after the round-trip archive has passed Apple/native checks.
            ASSETS.mkdir(mode=0o700)
            shutil.copyfile(final_zip, ASSETS / final_zip.name)
            (ASSETS / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
            (ASSETS / "SHA256SUMS").write_text(f"{checksum}  {final_zip.name}\n")
        finally:
            for args in (
                ["security", "list-keychains", "-d", "user", "-s"] + previous_keychains,
                ["security", "delete-keychain", str(keychain)] if created_keychain else None,
            ):
                if args is not None:
                    try:
                        run(args)
                    except ReleaseError:
                        cleanup_error = "Temporary release keychain cleanup failed; this job must not publish."
            if cleanup_error:
                raise ReleaseError(cleanup_error)


if __name__ == "__main__":
    def interrupted(signum, frame):
        raise ReleaseError("Release signing interrupted; no publication is authorized.")

    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        sign()
    except (ReleaseError, KeyError, ValueError, OSError) as error:
        print("Signing failed: " + (str(error) if isinstance(error, ReleaseError) else "Invalid or unavailable release configuration."), file=sys.stderr)
        sys.exit(1)
