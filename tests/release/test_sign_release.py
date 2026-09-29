import json
import io
import os
from pathlib import Path
import plistlib
import sys
import tempfile
import unittest
from unittest.mock import patch
from contextlib import redirect_stdout

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import release
import sign_release

COMMIT = "a" * 40
TEAM = "ABCDEFGHIJ"
IDENTITY = "A" * 40


class SignTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.app = self.root / "PR Sniper.app"
        (self.app / "Contents/MacOS").mkdir(parents=True)
        (self.app / "Contents/MacOS/pr-sniper").write_bytes(b"\xcf\xfa\xed\xfe fixture")
        self.info = {"CFBundleIdentifier": release.BUNDLE_ID, "CFBundleShortVersionString": "0.1.0",
                     "CFBundleVersion": "0.1.0", "CFBundleExecutable": "pr-sniper", "LSMinimumSystemVersion": "13.5"}
        self.write_info()
        self.calls = []
        self.signature = (f"Identifier={release.BUNDLE_ID}\nInfo.plist entries=16\nTeamIdentifier={TEAM}\n"
                          "Authority=Developer ID Application: Fixture (ABCDEFGHIJ)\n"
                          "Authority=Developer ID Certification Authority\nAuthority=Apple Root CA\n"
                          "Timestamp=fixture\nflags=0x10000(runtime)\n")
        self.entitlements = {}

    def tearDown(self):
        self.temp.cleanup()

    def write_info(self):
        (self.app / "Contents/Info.plist").write_bytes(plistlib.dumps(self.info))

    def verifier(self, args, timeout=120):
        self.calls.append(args)
        if args[0] == "lipo":
            return b"arm64\n"
        if "-dv" in args:
            return self.signature.encode()
        if "--entitlements" in args:
            return plistlib.dumps(self.entitlements)
        return b""

    def test_real_metadata_validation_requires_correct_version_arch_identity_and_chain(self):
        with patch.object(sign_release, "run", self.verifier):
            sign_release.verify_app(self.app, "0.1.0", TEAM)
            with self.assertRaises(release.ReleaseError):
                sign_release.verify_app(self.app, "0.1.0", "OTHERTEAM0")
            self.info["CFBundleShortVersionString"] = "0.2.0"
            self.write_info()
            with self.assertRaises(release.ReleaseError):
                sign_release.verify_app(self.app, "0.1.0", TEAM)

    def test_ad_hoc_signature_or_debug_entitlements_cannot_pass_distribution_gate(self):
        with patch.object(sign_release, "run", self.verifier):
            self.entitlements = {"com.apple.security.get-task-allow": True}
            with self.assertRaises(release.ReleaseError):
                sign_release.verify_app(self.app, "0.1.0", TEAM)
            self.entitlements = {}
            self.signature += "Signature=adhoc\n"
            with self.assertRaises(release.ReleaseError):
                sign_release.verify_app(self.app, "0.1.0", TEAM)

    def test_unexpected_native_helpers_and_links_need_explicit_signing_support(self):
        extra = self.app / "Contents/MacOS/helper"
        with patch.object(sign_release, "run", self.verifier):
            extra.write_bytes(b"\xcf\xfa\xed\xfe extra native code")
            with self.assertRaisesRegex(release.ReleaseError, "nested native"):
                sign_release.verify_app(self.app, "0.1.0", TEAM)
            extra.unlink()
            extra.symlink_to("/bin/sh")
            with self.assertRaisesRegex(release.ReleaseError, "symlink"):
                sign_release.verify_app(self.app, "0.1.0", TEAM)

    def run_sign(self, failure=None):
        output = self.root / "assets"
        values = {"APPLE_CERTIFICATE_PASSWORD": "fake export password", "APPLE_NOTARY_KEY_P8": "fake private key",
                  "APPLE_NOTARY_KEY_ID": "ABCDEFGHIJ", "APPLE_NOTARY_ISSUER_ID": "00000000-0000-0000-0000-000000000001",
                  "APPLE_TEAM_ID": TEAM, "APPLE_SIGNING_IDENTITY": IDENTITY}

        previous = ["/Users/runner/Library/Keychains/login.keychain-db", "/Library/Keychains/System.keychain"]
        keychains = previous[:]

        def native(args, timeout=120):
            nonlocal keychains
            self.calls.append(args)
            if args[:2] == ["git", "rev-parse"]:
                return COMMIT.encode()
            if args == ["security", "list-keychains", "-d", "user"]:
                return "\n".join(json.dumps(path) for path in keychains).encode()
            if args[:5] == ["security", "list-keychains", "-d", "user", "-s"]:
                if not ((failure == "search-list" and len(args[5:]) == 3)
                        or (failure == "restore" and args[5:] == previous)):
                    keychains = args[5:]
                return b""
            if args[:2] == ["codesign", "--force"]:
                self.assertEqual(keychains, [args[args.index("--keychain") + 1]] + previous)
            if args[:2] == ["security", "find-identity"]:
                return f'1) {IDENTITY if failure != "identity" else "B" * 40} "Developer ID Application: Fixture ({TEAM})"'.encode()
            if args[:3] == ["xcrun", "notarytool", "submit"]:
                if failure == "timeout":
                    raise release.ReleaseError("Notarization outcome unknown")
                return json.dumps({"id": "00000000-0000-0000-0000-000000000001",
                                   "status": "Invalid" if failure == "notary" else "Accepted"}).encode()
            if args[:2] == ["ditto", "-c"]:
                Path(args[-1]).write_bytes(b"synthetic final archive")
            if failure == "cleanup" and args[:2] == ["security", "delete-keychain"]:
                raise release.ReleaseError("cleanup failure")
            return b""

        environment = {"GITHUB_REPOSITORY": release.REPOSITORY, "GITHUB_REF_TYPE": "tag",
                       "GITHUB_REF_NAME": "v0.1.0", "SOURCE_SHA": COMMIT,
                       "RUNNER_TEMP": str(self.root), "RUNNER_ENVIRONMENT": "github-hosted"}
        with patch.dict(os.environ, environment), patch.object(sign_release.platform, "system", return_value="Darwin"), \
                patch.object(sign_release.platform, "machine", return_value="arm64"), \
                patch.object(sign_release, "configuration", return_value=(values, b"fake P12")), \
                patch.object(sign_release, "config_version", return_value="0.1.0"), \
                patch.object(sign_release, "verify_app"), patch.object(sign_release, "run", native), \
                patch.object(sign_release, "ASSETS", output):
            if failure:
                with self.assertRaises(release.ReleaseError):
                    sign_release.sign()
            else:
                sign_release.sign()
        return output

    def test_complete_signing_path_restores_keychains_and_exports_only_final_public_assets(self):
        output = self.run_sign()
        self.assertEqual({p.name for p in output.iterdir()}, {release.filename("0.1.0"), "manifest.json", "SHA256SUMS"})
        self.assertTrue(any(args[:3] == ["xcrun", "stapler", "staple"] for args in self.calls))
        self.assertTrue(any(args[0] == "spctl" for args in self.calls))
        self.assertFalse(any("--deep" in args for args in self.calls))
        restore = [args for args in self.calls if args[:5] == ["security", "list-keychains", "-d", "user", "-s"]]
        self.assertEqual(len(restore), 2)
        self.assertEqual(restore[0][6:], restore[-1][5:])
        self.assertEqual(restore[-1][5:], ["/Users/runner/Library/Keychains/login.keychain-db", "/Library/Keychains/System.keychain"])
        self.assertFalse(list(self.root.glob("pr-sniper-sign-*")))

    def test_rejected_identity_notarization_timeout_or_cleanup_failure_cannot_report_success(self):
        for failure in ["identity", "notary", "timeout", "cleanup", "search-list", "restore"]:
            with self.subTest(failure=failure):
                self.calls.clear()
                output = self.root / "assets"
                if output.exists():
                    for path in output.iterdir():
                        path.unlink()
                    output.rmdir()
                self.run_sign(failure)
                self.assertTrue(any(args[:2] == ["security", "delete-keychain"] for args in self.calls))
                if failure not in ("cleanup", "restore"):
                    self.assertFalse(output.exists())
                if failure in ("identity", "search-list"):
                    self.assertFalse(any(args[:2] == ["codesign", "--force"] for args in self.calls))
                self.assertFalse(list(self.root.glob("pr-sniper-sign-*")))

    def test_native_subprocess_environment_does_not_inherit_publishing_credentials(self):
        with patch.dict(os.environ, {"APPLE_NOTARY_KEY_P8": "never passed", "GITHUB_TOKEN": "never passed",
                                    "HOMEBREW_TAP_TOKEN": "never passed", "PATH": "/usr/bin"}), \
                patch.object(sign_release.subprocess, "run") as native:
            native.return_value.returncode = 0
            native.return_value.stdout = b""
            native.return_value.stderr = b""
            sign_release.run(["ditto", "-c", "fixture"])
            child = native.call_args.kwargs["env"]
            self.assertNotIn("APPLE_NOTARY_KEY_P8", child)
            self.assertNotIn("GITHUB_TOKEN", child)
            self.assertNotIn("HOMEBREW_TAP_TOKEN", child)
            self.assertEqual(child["PATH"], "/usr/bin")

    def test_native_errors_identify_operation_without_echoing_output_or_arguments(self):
        secret = "DO-NOT-PRINT-PRIVATE-MATERIAL"
        for args, expected in [
            (["codesign", "--force", "--sign", secret, "app"], "codesign/sign"),
            (["codesign", "--verify", "--strict", "app"], "codesign/verify"),
            (["security", "import", secret, "-P", secret], "security/import"),
        ]:
            with self.subTest(operation=expected), patch.object(sign_release.subprocess, "run") as native:
                native.return_value.returncode = 1
                native.return_value.stdout = secret.encode()
                native.return_value.stderr = b"errSecInternalComponent: " + secret.encode()
                output = io.StringIO()
                with redirect_stdout(output), self.assertRaises(release.ReleaseError) as failure:
                    sign_release.run(args)
                message = str(failure.exception)
                self.assertIn(expected, message)
                self.assertIn("exit=1", message)
                self.assertIn("category=security-internal-component", message)
                self.assertNotIn(secret, message + output.getvalue())

    def test_search_list_readback_and_parsing_fail_explicitly(self):
        with patch.object(sign_release, "run", return_value=b'"/different/keychain"'):
            with self.assertRaisesRegex(release.ReleaseError, "not confirmed"):
                sign_release.set_keychain_search_list(["/expected/keychain"])
        for result in [b'"unterminated', b'"relative/keychain"', b'"/path\nwith-control"']:
            with self.subTest(result=result), patch.object(sign_release, "run", return_value=result), \
                    self.assertRaises(release.ReleaseError):
                sign_release.keychain_search_list()


if __name__ == "__main__":
    unittest.main()
