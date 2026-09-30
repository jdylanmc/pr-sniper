import base64
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import release
import sign_release

ROOT = Path(__file__).resolve().parents[2]
COMMIT = "a" * 40


class Api:
    def __init__(self):
        self.sha = COMMIT
        self.ci = {"id": 1, "head_sha": COMMIT, "head_branch": "main", "event": "push",
                   "status": "completed", "conclusion": "success"}
        self.release = None
        self.latest = None
        self.assets = []
        self.bytes = {}
        self.cask = None
        self.requests = []
        self.wrong_digest = False
        self.corrupt_download = False
        self.unknown_upload = False
        self.confirm_dispatch = True

    def request(self, path, method="GET", value=None, missing=False, data=None, content_type=None):
        self.requests.append((method, path, value))
        if path.endswith("/git/ref/tags/v0.1.0"):
            return {"object": {"type": "commit", "sha": self.sha}}
        if path.endswith("/git/ref/heads/main"):
            return {"object": {"type": "commit", "sha": COMMIT}}
        if "/compare/" in path:
            return {"status": "identical"}
        if path.endswith("/releases/latest"):
            return self.latest
        if "/releases/tags/" in path:
            return self.release
        if method == "POST" and path.endswith("/releases"):
            self.release = {"id": 2, **value}
            return self.release
        if method == "PATCH" and path.endswith("/releases/2"):
            self.release.update(value)
            return self.release
        if method == "POST" and "/assets?name=" in path:
            name = path.split("name=")[1]
            self.bytes[name] = data
            asset = {"name": name, "state": "uploaded", "size": len(data),
                     "digest": "sha256:" + ("0" * 64 if self.wrong_digest else hashlib.sha256(data).hexdigest()),
                     "browser_download_url": f"https://github.com/{release.REPOSITORY}/releases/download/v0.1.0/{name}"}
            self.assets.append(asset)
            if self.unknown_upload:
                raise release.ReleaseError("Unknown upload outcome")
            return asset
        if "/contents/Casks/pr-sniper.rb" in path:
            assert method == "GET", "Parent must not directly write the cask."
            return self.cask
        if path.endswith("/dispatches") and method == "POST":
            if self.confirm_dispatch:
                payload = value["client_payload"]
                text = release.cask(release.version(payload["tag"]), payload["sha256"])
                self.cask = {"type": "file", "encoding": "base64", "content": base64.b64encode(text.encode()).decode(), "sha": "new"}
            return None
        raise AssertionError((method, path))

    def pages(self, path, field=None):
        if "/actions/workflows/" in path:
            return [self.ci]
        if "/jobs" in path:
            return [{"name": "macos", "conclusion": self.ci["conclusion"]}]
        if path.endswith("/assets"):
            return self.assets
        raise AssertionError(path)

    def download(self, url):
        return b"tampered" if self.corrupt_download else self.bytes[url.rsplit("/", 1)[1]]


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.directory = Path(self.temp.name)
        self.env = patch.dict(os.environ, {"SOURCE_SHA": COMMIT, "GITHUB_REF_NAME": "v0.1.0"})
        self.env.start()
        self.api = Api()
        self.downloader = patch.object(release, "public_bytes", self.api.download)
        self.downloader.start()

    def tearDown(self):
        self.downloader.stop()
        self.env.stop()
        self.temp.cleanup()

    def assets(self):
        archive = b"fixture notarized ZIP, never executable"
        manifest = {"version": "0.1.0", "tag": "v0.1.0", "source_sha": COMMIT,
                    "filename": release.filename("0.1.0"), "sha256": hashlib.sha256(archive).hexdigest(),
                    "architecture": "arm64", "bundle_id": release.BUNDLE_ID,
                    "team_id": "ABCDEFGHIJ", "notarization_id": "12345678-1234-1234-1234-123456789012"}
        (self.directory / manifest["filename"]).write_bytes(archive)
        (self.directory / "manifest.json").write_text(json.dumps(manifest))
        (self.directory / "SHA256SUMS").write_bytes(f"{manifest['sha256']}  {manifest['filename']}\n".encode())
        return manifest

    def test_versions_are_exact_and_all_checked_in_manifests_agree(self):
        self.assertEqual(release.config_version(ROOT), json.loads((ROOT / "package.json").read_text())["version"])
        for tag in ["v01.0.0", "1.0.0", "v1.0.0-beta", "v1.0", "v1.2.3\n", "v1.2.3;echo bad"]:
            with self.subTest(tag=tag), self.assertRaises(release.ReleaseError):
                release.version(tag)

    def test_gate_requires_exact_main_push_ci_not_pr_or_other_commit(self):
        with patch.object(release, "command", return_value=COMMIT), \
                patch.object(release, "config_version", return_value="0.1.0"):
            self.assertEqual(release.gate(self.api, "v0.1.0", ROOT)["sha"], COMMIT)
            for field, value in [("head_sha", "b" * 40), ("head_branch", "feature"),
                                 ("event", "pull_request"), ("conclusion", "failure"), ("status", "in_progress")]:
                old = self.api.ci[field]
                self.api.ci[field] = value
                with self.subTest(field=field), self.assertRaises(release.ReleaseError):
                    release.gate(self.api, "v0.1.0", ROOT)
                self.api.ci[field] = old
            self.api.sha = "b" * 40
            with self.assertRaises(release.ReleaseError):
                release.gate(self.api, "v0.1.0", ROOT)

    def test_gate_checks_main_ancestry_and_latest_successful_attempt(self):
        with patch.object(release, "command", return_value=COMMIT), \
                patch.object(release, "config_version", return_value="0.1.0"):
            original = self.api.request
            with patch.object(self.api, "request", side_effect=lambda path, **kw:
                              {"status": "diverged"} if "/compare/" in path else original(path, **kw)):
                with self.assertRaises(release.ReleaseError):
                    release.gate(self.api, "v0.1.0", ROOT)
            original_pages = self.api.pages
            with patch.object(self.api, "pages", side_effect=lambda path, field=None:
                              [self.api.ci, {**self.api.ci, "id": 2, "conclusion": "failure"}]
                              if field else original_pages(path, field)):
                with self.assertRaises(release.ReleaseError):
                    release.gate(self.api, "v0.1.0", ROOT)

    def test_manifest_is_bound_to_tag_commit_architecture_and_final_bytes(self):
        manifest = self.assets()
        self.assertEqual(release.metadata(self.directory), manifest)
        for field, value in [("architecture", "x86_64"), ("source_sha", "b" * 40),
                             ("tag", "v0.2.0"), ("bundle_id", "other"), ("sha256", "0" * 64),
                             ("filename", "../escape.zip"), ("notarization_id", "")]:
            (self.directory / "manifest.json").write_text(json.dumps({**manifest, field: value}))
            with self.subTest(field=field), self.assertRaises(release.ReleaseError):
                release.metadata(self.directory)

    def test_published_asset_bytes_are_verified_before_tap_write(self):
        manifest = self.assets()
        release.publish(self.api, self.directory)
        self.assertFalse(self.api.release["draft"])
        tap = Api()
        release.tap_update(self.api, tap, self.directory)
        self.assertEqual(len([r for r in tap.requests if r[0] == "POST"]), 1)
        text = base64.b64decode(tap.cask["content"]).decode()
        self.assertEqual(text, release.cask("0.1.0", manifest["sha256"]))
        self.assertIn('depends_on arch: :arm64', text)
        self.assertIn('depends_on macos: :ventura', text)
        self.assertIn("Requires macOS 13.5 or later", text)
        self.assertNotIn("auto_updates", text)
        self.assertNotIn("zap", text)

    def test_identical_published_release_and_tap_reruns_are_noops(self):
        self.assets()
        release.publish(self.api, self.directory)
        writes = len([r for r in self.api.requests if r[0] != "GET"])
        release.publish(self.api, self.directory)
        self.assertEqual(len([r for r in self.api.requests if r[0] != "GET"]), writes)
        tap = Api()
        release.tap_update(self.api, tap, self.directory)
        release.tap_update(self.api, tap, self.directory)
        self.assertEqual(len([r for r in tap.requests if r[0] == "POST"]), 1)
        self.assertFalse(any(r[0] == "PUT" for r in tap.requests))

    def test_interrupted_upload_is_not_replaced_or_silently_published(self):
        self.assets()
        self.api.unknown_upload = True
        with self.assertRaises(release.ReleaseError):
            release.publish(self.api, self.directory)
        self.assertTrue(self.api.release["draft"])
        with self.assertRaisesRegex(release.ReleaseError, "draft"):
            release.publish(self.api, self.directory)
        self.assertEqual(len(self.api.assets), 1)

    def test_wrong_upload_digest_leaves_draft_and_never_updates_tap(self):
        self.assets()
        self.api.wrong_digest = True
        with self.assertRaisesRegex(release.ReleaseError, "digest"):
            release.publish(self.api, self.directory)
        self.assertTrue(self.api.release["draft"])

    def test_tampered_published_bytes_or_moved_tag_prevent_tap_update(self):
        self.assets()
        release.publish(self.api, self.directory)
        tap = Api()
        self.api.corrupt_download = True
        with self.assertRaises(release.ReleaseError):
            release.tap_update(self.api, tap, self.directory)
        self.assertIsNone(tap.cask)
        self.api.corrupt_download = False
        self.api.sha = "b" * 40
        with self.assertRaises(release.ReleaseError):
            release.tap_update(self.api, tap, self.directory)
        self.assertIsNone(tap.cask)

    def test_tap_cannot_downgrade_or_rewrite_existing_version(self):
        self.assets()
        release.publish(self.api, self.directory)
        for existing in ["0.2.0", "0.1.0"]:
            tap = Api()
            tap.cask = {"type": "file", "encoding": "base64", "sha": "old",
                        "content": base64.b64encode(release.cask(existing, "0" * 64).encode()).decode()}
            with self.assertRaises(release.ReleaseError):
                release.tap_update(self.api, tap, self.directory)
            self.assertFalse(any(r[0] == "PUT" for r in tap.requests))

    def test_newer_latest_release_cannot_be_replaced(self):
        self.assets()
        self.api.latest = {"tag_name": "v0.2.0"}
        with self.assertRaisesRegex(release.ReleaseError, "older"):
            release.publish(self.api, self.directory)
        self.assertIsNone(self.api.release)

    def test_unsafe_cask_inputs_cannot_become_ruby_code(self):
        for ver, digest in [('1.0.0"\nsystem("bad")', "0" * 64), ("1.0.0", '";system("bad")')]:
            with self.assertRaises(release.ReleaseError):
                release.cask(ver, digest)

    def test_accepted_dispatch_without_matching_cask_is_not_success_or_redispatched(self):
        self.assets()
        release.publish(self.api, self.directory)
        tap = Api()
        tap.confirm_dispatch = False
        with patch.object(release.time, "monotonic", side_effect=[0, 0, 601]), \
                patch.object(release.time, "sleep"), self.assertRaisesRegex(release.ReleaseError, "unconfirmed"):
            release.tap_update(self.api, tap, self.directory)
        self.assertEqual(len([r for r in tap.requests if r[0] == "POST"]), 1)
        self.assertFalse(any(r[0] == "PUT" for r in tap.requests))

    def test_repository_dispatch_accepts_only_its_expected_empty_204_response(self):
        with patch.object(release.urllib.request, "build_opener") as opener, \
                patch.object(release.json, "load") as load:
            response = opener.return_value.open.return_value.__enter__.return_value
            response.status = 204
            result = release.Github("synthetic-token").request(
                f"/repos/{release.TAP}/dispatches", "POST",
                {"event_type": "pr-sniper-release", "client_payload": {"tag": "v0.1.0"}},
            )
            self.assertIsNone(result)
            load.assert_not_called()
            request = opener.return_value.open.call_args.args[0]
            self.assertEqual(request.full_url, f"https://api.github.com/repos/{release.TAP}/dispatches")
            self.assertEqual(request.get_method(), "POST")

    def test_native_configuration_does_not_accept_missing_wrong_or_ambiguous_identity(self):
        values = {"APPLE_CERTIFICATE_P12": base64.b64encode(b"fixture").decode(),
                  "APPLE_CERTIFICATE_PASSWORD": "fixture-password",
                  "APPLE_NOTARY_KEY_P8": "-----BEGIN PRIVATE KEY-----\nfixture\n-----END PRIVATE KEY-----",
                  "APPLE_NOTARY_KEY_ID": "ABCDEFGHIJ", "APPLE_NOTARY_ISSUER_ID": "12345678-1234-1234-1234-123456789012",
                  "APPLE_TEAM_ID": "ABCDEFGHIJ", "APPLE_SIGNING_IDENTITY": "A" * 40}
        with patch.dict(os.environ, values):
            self.assertEqual(sign_release.configuration()[1], b"fixture")
            for key, bad in [("APPLE_SIGNING_IDENTITY", "Developer ID Application: Duplicate name"),
                             ("APPLE_TEAM_ID", ""), ("APPLE_CERTIFICATE_P12", "not base64!"),
                             ("APPLE_NOTARY_KEY_P8", "/path/to/key.p8")]:
                with self.subTest(key=key), patch.dict(os.environ, {key: bad}), self.assertRaises(release.ReleaseError):
                    sign_release.configuration()

    def test_tap_preflight_requires_exact_write_scope_and_existing_validation(self):
        repository = {"full_name": release.TAP, "default_branch": "main", "private": False,
                      "permissions": {"push": True}}
        with patch.object(self.api, "request", side_effect=[repository, {"type": "file"}]):
            release.tap_ready(self.api)
        for wrong in [{**repository, "permissions": {"push": False}},
                      {**repository, "full_name": "jdylanmc/homebrew-notch"},
                      {**repository, "default_branch": "other"}]:
            with patch.object(self.api, "request", return_value=wrong), self.assertRaises(release.ReleaseError):
                release.tap_ready(self.api)


if __name__ == "__main__":
    unittest.main()
