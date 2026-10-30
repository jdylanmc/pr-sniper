import base64
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

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

    def test_gate_accepts_release_dispatched_main_ci_only_on_main_or_its_pin_branch(self):
        with patch.object(release, "command", return_value=COMMIT), \
                patch.object(release, "config_version", return_value="0.1.0"):
            self.api.ci["event"] = "workflow_dispatch"
            for branch, ok in [("main", True), ("changesets/ci-v0.1.0", True),
                               ("changesets/ci-v0.2.0", False), ("changesets/release-v0.1.0", False)]:
                self.api.ci["head_branch"] = branch
                with self.subTest(branch=branch):
                    if ok:
                        self.assertEqual(release.gate(self.api, "v0.1.0", ROOT)["sha"], COMMIT)
                    else:
                        with self.assertRaises(release.ReleaseError):
                            release.gate(self.api, "v0.1.0", ROOT)
            self.api.ci.update(event="push", head_branch="changesets/ci-v0.1.0")
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

    def test_windows_publication_adds_exact_native_and_installer_gates_without_changing_macos(self):
        windows = {**self.api.ci, "id": 10}
        jobs = [{"name": name, "conclusion": "success"}
                for name in ("windows", "windows-installer-acceptance")]
        original_pages = self.api.pages

        def pages(path, field=None):
            if "windows.yml" in path:
                return [windows]
            if "/runs/10/jobs" in path:
                return jobs
            return original_pages(path, field)

        with patch.object(release, "command", return_value=COMMIT), \
                patch.object(release, "config_version", return_value="0.1.0"), \
                patch.object(self.api, "pages", side_effect=pages):
            self.assertEqual(release.gate(self.api, "v0.1.0", ROOT, require_windows=True)["sha"], COMMIT)
            for field, value in [("head_sha", "b" * 40), ("event", "pull_request"),
                                 ("head_branch", "feature"), ("status", "in_progress"),
                                 ("conclusion", "failure")]:
                previous = windows[field]
                windows[field] = value
                with self.subTest(field=field), self.assertRaises(release.ReleaseError):
                    release.gate(self.api, "v0.1.0", ROOT, require_windows=True)
                self.assertEqual(release.gate(self.api, "v0.1.0", ROOT)["sha"], COMMIT)
                windows[field] = previous
            for job in jobs:
                job["conclusion"] = "skipped"
                with self.assertRaises(release.ReleaseError):
                    release.gate(self.api, "v0.1.0", ROOT, require_windows=True)
                job["conclusion"] = "success"
            with patch.object(self.api, "pages", side_effect=lambda path, field=None:
                              [windows, {**windows, "id": 11, "conclusion": "failure"}]
                              if "windows.yml" in path else pages(path, field)):
                with self.assertRaises(release.ReleaseError):
                    release.gate(self.api, "v0.1.0", ROOT, require_windows=True)

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
        with patch.object(release.time, "monotonic", side_effect=[0, 0, release.TAP_ATTEMPT_SECONDS + 1]), \
                patch.object(release.time, "sleep"), self.assertRaisesRegex(release.ReleaseError, "unconfirmed"):
            release.tap_update(self.api, tap, self.directory)
        self.assertEqual(len([r for r in tap.requests if r[0] == "POST"]), 1)
        self.assertFalse(any(r[0] == "PUT" for r in tap.requests))

    def tap_with_runs(self, failed_steps):
        """Tap whose publisher runs fail at `failed_steps[i]` per dispatch; None publishes."""
        tap = Api()
        tap.confirm_dispatch = False
        runs, original_request = [], tap.request

        def request(path, method="GET", value=None, **kwargs):
            result = original_request(path, method, value, **kwargs)
            if path.endswith("/dispatches"):
                step = failed_steps[len(runs)]
                runs.append({"id": len(runs) + 1, "event": "repository_dispatch", "status": "completed",
                             "conclusion": "failure" if step else "success", "step": step})
                if step is None:
                    payload = value["client_payload"]
                    text = release.cask(release.version(payload["tag"]), payload["sha256"])
                    tap.cask = {"type": "file", "encoding": "base64",
                                "content": base64.b64encode(text.encode()).decode(), "sha": "new"}
            return result

        def pages(path, field=None):
            if "/actions/workflows/publish.yml/runs" in path:
                return runs[-1:]
            run = runs[int(path.split("/runs/")[1].split("/")[0]) - 1]
            return [{"name": "publish", "steps": [{"name": "Validate", "conclusion": "success"},
                                                  {"name": run["step"], "conclusion": "failure"}]}]
        tap.request, tap.pages = request, pages
        return tap

    def test_tap_redispatches_same_version_only_after_install_verification_failure(self):
        self.assets()
        release.publish(self.api, self.directory)
        tap = self.tap_with_runs([release.TAP_VERIFY_STEP, release.TAP_VERIFY_STEP, None])
        with patch.object(release.time, "sleep") as sleep:
            release.tap_update(self.api, tap, self.directory)
        dispatches = [r for r in tap.requests if r[0] == "POST"]
        self.assertEqual(len(dispatches), 3)
        self.assertEqual(len({json.dumps(r[2], sort_keys=True) for r in dispatches}), 1)
        sleep.assert_any_call(release.TAP_BACKOFF[0])
        sleep.assert_any_call(release.TAP_BACKOFF[1])
        self.assertFalse(any(r[0] == "PUT" for r in tap.requests))

    def test_tap_retry_is_bounded_and_other_failures_never_redispatch(self):
        self.assets()
        release.publish(self.api, self.directory)
        tap = self.tap_with_runs([release.TAP_VERIFY_STEP] * 10)
        with patch.object(release.time, "sleep"), self.assertRaisesRegex(release.ReleaseError, "4 times"):
            release.tap_update(self.api, tap, self.directory)
        self.assertEqual(len([r for r in tap.requests if r[0] == "POST"]), len(release.TAP_BACKOFF) + 1)
        tap = self.tap_with_runs(["Validate immutable public release and prepare cask"])
        with patch.object(release.time, "sleep"), self.assertRaisesRegex(release.ReleaseError, "outside"):
            release.tap_update(self.api, tap, self.directory)
        self.assertEqual(len([r for r in tap.requests if r[0] == "POST"]), 1)

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

    def build_nupkg(self, package_path, manifest, *, nuspec_overrides=None, installer_overrides=None,
                    extra_entries=None, script_overrides=None, date_time=(2026, 1, 1, 0, 0, 0)):
        nuspec_meta = {
            "id": "pr-sniper",
            "version": manifest["version"],
            "licenseUrl": f"https://github.com/{release.REPOSITORY}/blob/{manifest['source_sha']}/LICENSE",
            "dependency_id": "webview2-runtime",
            "dependency_version": release.CHOCO_WEBVIEW2_VERSION_RANGE,
            "description": (
                "UNSIGNED PUBLIC RELEASE: No Authenticode publisher identity. PR Sniper monitors repositories."
                if manifest.get("trust_mode") == "unsigned"
                else "PR Sniper monitors configured repositories and prepares human-owned pull request review work."
            ),
        }
        if nuspec_overrides:
            nuspec_meta.update(nuspec_overrides)
        nuspec_xml = (
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<package xmlns="http://schemas.microsoft.com/packaging/2015/06/nuspec.xsd">\n'
            "  <metadata>\n"
            f"    <id>{nuspec_meta['id']}</id>\n"
            f"    <version>{nuspec_meta['version']}</version>\n"
            f"    <licenseUrl>{nuspec_meta['licenseUrl']}</licenseUrl>\n"
            f'    <dependencies><dependency id="{nuspec_meta["dependency_id"]}" '
            f'version="{nuspec_meta["dependency_version"]}" /></dependencies>\n'
            f"    <description>{nuspec_meta['description']}</description>\n"
            "  </metadata>\n"
            "</package>\n"
        )
        installer_json = {
            "mode": "PublicUnsigned" if manifest.get("trust_mode") == "unsigned" else "Public",
            "package_id": "pr-sniper",
            "version": manifest["version"],
            "commit": manifest["source_sha"],
            "target": "x86_64-pc-windows-msvc",
            "sha256": manifest["sha256"],
            "url": f"https://github.com/{release.REPOSITORY}/releases/download/{manifest['tag']}/{manifest['filename']}",
            "signer_thumbprint": manifest.get("signer_thumbprint"),
            "signer_subject": manifest.get("signer_subject"),
        }
        if installer_overrides:
            installer_json.update(installer_overrides)
        scripts = {
            "tools/chocolateyinstall.ps1": (ROOT / "packaging" / "chocolatey" / "chocolateyinstall.ps1").read_bytes(),
            "tools/chocolateyuninstall.ps1": (ROOT / "packaging" / "chocolatey" / "chocolateyuninstall.ps1").read_bytes(),
            "tools/removal-state.ps1": (ROOT / "packaging" / "chocolatey" / "removal-state.ps1").read_bytes(),
            "tools/windows-signature.ps1": (ROOT / "scripts" / "windows-signature.ps1").read_bytes(),
            "tools/windows-webview2.ps1": (ROOT / "scripts" / "windows-webview2.ps1").read_bytes(),
        }
        if script_overrides:
            scripts.update(script_overrides)
        with zipfile.ZipFile(package_path, "w") as archive:
            for entry_name, payload in [
                ("pr-sniper.nuspec", nuspec_xml),
                ("tools/installer.json", json.dumps(installer_json)),
                ("tools/installer.exe.ignore", ""),
                *scripts.items(),
            ]:
                archive.writestr(zipfile.ZipInfo(entry_name, date_time=date_time), payload)
            for entry_name, payload in (extra_entries or {}).items():
                if isinstance(payload, tuple):
                    info, data = payload
                    archive.writestr(info, data)
                else:
                    archive.writestr(zipfile.ZipInfo(entry_name, date_time=date_time), payload)
        return package_path

    def windows_assets(self, directory=None, trust_mode="unsigned", **overrides):
        target = directory or self.directory
        installer = b"fixture Windows NSIS setup executable, never executed"
        manifest = {
            "version": "0.1.0",
            "tag": "v0.1.0",
            "source_sha": COMMIT,
            "filename": release.windows_filename("0.1.0"),
            "sha256": hashlib.sha256(installer).hexdigest(),
            "architecture": "x86_64",
            "bundle_id": release.BUNDLE_ID,
            "trust_mode": trust_mode,
        }
        if trust_mode == "signed":
            manifest["signer_thumbprint"] = "A" * 40
            manifest["signer_subject"] = "CN=Dylan McCurry"
        manifest.update(overrides)
        (target / release.windows_filename("0.1.0")).write_bytes(installer)
        (target / "windows-manifest.json").write_text(json.dumps(manifest))
        (target / "WINDOWS-SHA256SUMS").write_bytes(
            f"{manifest['sha256']}  {manifest['filename']}\n".encode()
        )
        self.build_nupkg(target / "pr-sniper.0.1.0.nupkg", manifest)
        return manifest

    def windows_pages(self, path, field=None):
        if "windows.yml" in path:
            return [{**self.api.ci, "id": 10}]
        if "/runs/10/jobs" in path:
            return [{"name": "windows", "conclusion": "success"},
                    {"name": "windows-installer-acceptance", "conclusion": "success"}]
        if "/actions/workflows/" in path:
            return [self.api.ci]
        if "/jobs" in path:
            return [{"name": "macos", "conclusion": self.api.ci["conclusion"]}]
        if path.endswith("/assets"):
            return self.api.assets
        raise AssertionError(path)

    def test_windows_metadata_validates_unsigned_and_signed_manifests_and_checksums(self):
        self.assertEqual(release.windows_filename("0.1.0"), "pr-sniper-0.1.0-x64-setup.exe")
        for invalid in ["01.0.0", "1.0.0-beta", "1.0", "../bad"]:
            with self.subTest(invalid=invalid), self.assertRaises(release.ReleaseError):
                release.windows_filename(invalid)
        unsigned = self.windows_assets(trust_mode="unsigned")
        self.assertEqual(release.windows_metadata(self.directory), unsigned)
        signed = self.windows_assets(trust_mode="signed")
        self.assertEqual(release.windows_metadata(self.directory), signed)
        for bad_overrides in [
            {"trust_mode": "unsigned", "signer_thumbprint": "A" * 40, "signer_subject": None},
            {"trust_mode": "unsigned", "signer_subject": "CN=Unexpected"},
            {"trust_mode": "signed", "signer_thumbprint": "a" * 40},
            {"trust_mode": "signed", "signer_thumbprint": "ABCD"},
            {"trust_mode": "signed", "signer_subject": "   "},
            {"trust_mode": "ad-hoc"},
            {"architecture": "arm64"},
            {"source_sha": "b" * 40},
            {"tag": "v0.2.0"},
            {"bundle_id": "other"},
            {"sha256": "0" * 64},
            {"filename": "pr-sniper-0.1.0-aarch64-apple-darwin.zip"},
            {"unexpected": "field"},
        ]:
            with self.subTest(overrides=bad_overrides):
                self.windows_assets(**{"trust_mode": "signed", **bad_overrides})
                with self.assertRaises(release.ReleaseError):
                    release.windows_metadata(self.directory)
        self.windows_assets(trust_mode="unsigned")
        (self.directory / "WINDOWS-SHA256SUMS").write_text("bad  pr-sniper-0.1.0-x64-setup.exe\n")
        with self.assertRaises(release.ReleaseError):
            release.windows_metadata(self.directory)

    def test_stage_windows_release_promotes_verified_candidate_and_rejects_mismatches(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            candidate = base / "candidate"
            candidate.mkdir()
            installer = b"fixture Windows NSIS setup executable, never executed"
            digest = hashlib.sha256(installer).hexdigest()
            name = release.windows_filename("0.1.0")
            (candidate / name).write_bytes(installer)
            meta = {
                "schema": 1,
                "version": "0.1.0",
                "commit": COMMIT,
                "target": "x86_64-pc-windows-msvc",
                "filename": name,
                "sha256": digest,
                "application_sha256": "1" * 64,
                "standalone_application_sha256": "2" * 64,
                "bundle_id": release.BUNDLE_ID,
                "distribution": "unsigned-ci-candidate-not-a-public-release",
                "signed": False,
            }
            (candidate / "installer.json").write_text(json.dumps(meta), encoding="utf-8")
            dest = base / "staged"
            with patch.object(release, "config_version", return_value="0.1.0"):
                staged = release.stage_windows_release(candidate, dest, root=ROOT)
                self.assertEqual(staged, {
                    "version": "0.1.0",
                    "tag": "v0.1.0",
                    "source_sha": COMMIT,
                    "filename": name,
                    "sha256": digest,
                    "architecture": "x86_64",
                    "bundle_id": release.BUNDLE_ID,
                    "trust_mode": "unsigned",
                })
                self.assertEqual((dest / name).read_bytes(), installer)
                self.assertEqual(
                    (dest / "WINDOWS-SHA256SUMS").read_text(encoding="ascii"),
                    f"{digest}  {name}\n",
                )
                with self.assertRaisesRegex(release.ReleaseError, "already exists"):
                    release.stage_windows_release(candidate, dest, root=ROOT)

                for key, bad_val in [
                    ("commit", "b" * 40),
                    ("version", "0.1.1"),
                    ("sha256", "0" * 64),
                    ("signed", True),
                    ("distribution", "other"),
                    ("bundle_id", "other"),
                ]:
                    with self.subTest(key=key):
                        bad_dest = base / f"bad-{key}"
                        (candidate / "installer.json").write_text(
                            json.dumps({**meta, key: bad_val}), encoding="utf-8"
                        )
                        with self.assertRaises(release.ReleaseError):
                            release.stage_windows_release(candidate, bad_dest, root=ROOT)
                        self.assertFalse(bad_dest.exists())

    def test_publish_windows_preserves_macos_assets_and_is_idempotent_on_rerun(self):
        macos_manifest = self.assets()
        release.publish(self.api, self.directory)
        with tempfile.TemporaryDirectory() as win_tmp:
            win_dir = Path(win_tmp)
            win_manifest = self.windows_assets(win_dir, trust_mode="unsigned")
            with patch.object(release, "command", return_value=COMMIT), \
                    patch.object(release, "config_version", return_value="0.1.0"), \
                    patch.object(self.api, "pages", side_effect=self.windows_pages):
                release.publish_windows(self.api, win_dir)
                names = {asset["name"] for asset in self.api.assets}
                self.assertEqual(names, {
                    macos_manifest["filename"], "manifest.json", "SHA256SUMS",
                    win_manifest["filename"], "windows-manifest.json", "WINDOWS-SHA256SUMS",
                })
                writes_before = len([r for r in self.api.requests if r[0] != "GET"])
                release.publish_windows(self.api, win_dir)
                self.assertEqual(len([r for r in self.api.requests if r[0] != "GET"]), writes_before)

    def test_publish_windows_refuses_missing_draft_or_partial_release_and_bad_digest_or_gate(self):
        with tempfile.TemporaryDirectory() as win_tmp:
            win_dir = Path(win_tmp)
            win_manifest = self.windows_assets(win_dir, trust_mode="unsigned")
            with patch.object(release, "command", return_value=COMMIT), \
                    patch.object(release, "config_version", return_value="0.1.0"), \
                    patch.object(self.api, "pages", side_effect=self.windows_pages):
                with self.assertRaisesRegex(release.ReleaseError, "Published GitHub release"):
                    release.publish_windows(self.api, win_dir)
                self.api.release = {
                    "id": 2,
                    "tag_name": "v0.1.0",
                    "target_commitish": COMMIT,
                    "draft": False,
                    "prerelease": False,
                }
                self.api.assets = []
                uploads_before = len([r for r in self.api.requests if "/assets?name=" in r[1]])
                with self.assertRaisesRegex(release.ReleaseError, "Existing release assets are incomplete"):
                    release.publish_windows(self.api, win_dir)
                self.assertEqual(len([r for r in self.api.requests if "/assets?name=" in r[1]]), uploads_before)

                self.api.release = None
                self.assets()
                release.publish(self.api, self.directory)
                macos_assets = list(self.api.assets)

                self.api.assets = macos_assets[:-1]
                uploads_before = len([r for r in self.api.requests if "/assets?name=" in r[1]])
                with self.assertRaisesRegex(release.ReleaseError, "Existing release assets are incomplete"):
                    release.publish_windows(self.api, win_dir)
                self.assertEqual(len([r for r in self.api.requests if "/assets?name=" in r[1]]), uploads_before)
                self.api.assets = list(macos_assets)

                self.api.release["draft"] = True
                with self.assertRaisesRegex(release.ReleaseError, "Published GitHub release"):
                    release.publish_windows(self.api, win_dir)
                self.api.release["draft"] = False
                self.api.assets.append({
                    "name": "windows-manifest.json",
                    "state": "uploaded",
                    "size": 10,
                    "digest": "sha256:" + "0" * 64,
                    "browser_download_url": f"https://github.com/{release.REPOSITORY}/releases/download/v0.1.0/windows-manifest.json",
                })
                with self.assertRaisesRegex(release.ReleaseError, "Partial Windows release assets"):
                    release.publish_windows(self.api, win_dir)
                self.api.assets = [a for a in self.api.assets if a["name"] != "windows-manifest.json"]
                self.api.wrong_digest = True
                with self.assertRaisesRegex(release.ReleaseError, "digest"):
                    release.publish_windows(self.api, win_dir)
                self.api.wrong_digest = False
                self.api.assets = [a for a in self.api.assets if a["name"] not in (
                    win_manifest["filename"], "windows-manifest.json", "WINDOWS-SHA256SUMS"
                )]
                release.publish_windows(self.api, win_dir)
                all_six_assets = list(self.api.assets)
                self.api.assets = [a for a in all_six_assets if a["name"] in (
                    win_manifest["filename"], "windows-manifest.json", "WINDOWS-SHA256SUMS"
                )]
                with self.assertRaisesRegex(release.ReleaseError, "Release assets are incomplete"):
                    release.verify_windows_public(self.api, win_manifest, win_dir)
                self.api.assets = [a for a in all_six_assets if a["name"] in (
                    release.filename("0.1.0"), "manifest.json", "SHA256SUMS"
                )]
            with patch.object(release, "command", return_value=COMMIT), \
                    patch.object(release, "config_version", return_value="0.1.0"):
                with self.assertRaises(release.ReleaseError):
                    release.publish_windows(self.api, win_dir)

    def test_verify_choco_nupkg_validates_nuspec_metadata_scripts_and_rejects_tampering(self):
        with tempfile.TemporaryDirectory() as win_tmp:
            win_dir = Path(win_tmp)
            unsigned_manifest = self.windows_assets(win_dir, trust_mode="unsigned")
            nupkg = win_dir / "pr-sniper.0.1.0.nupkg"
            self.assertEqual(
                release.verify_choco_nupkg(nupkg, unsigned_manifest, root=ROOT),
                hashlib.sha256(nupkg.read_bytes()).hexdigest(),
            )
            signed_manifest = self.windows_assets(win_dir, trust_mode="signed")
            self.assertEqual(
                release.verify_choco_nupkg(nupkg, signed_manifest, root=ROOT),
                hashlib.sha256(nupkg.read_bytes()).hexdigest(),
            )
            self.build_nupkg(
                nupkg,
                unsigned_manifest,
                nuspec_overrides={"dependency_version": release.CHOCO_WEBVIEW2_MIN_VERSION},
            )
            self.assertEqual(
                release.verify_choco_nupkg(nupkg, unsigned_manifest, root=ROOT),
                hashlib.sha256(nupkg.read_bytes()).hexdigest(),
            )

            symlink_info = zipfile.ZipInfo("tools/symlink.ps1")
            symlink_info.external_attr = 0o120777 << 16
            for label, kwargs in [
                ("wrong-sha256", {"installer_overrides": {"sha256": "0" * 64}}),
                ("wrong-url", {"installer_overrides": {"url": "https://example.invalid/pr-sniper.exe"}}),
                ("wrong-mode-unsigned", {"installer_overrides": {"mode": "Public"}}),
                ("unexpected-signer-unsigned", {"installer_overrides": {"signer_subject": "CN=Unexpected"}}),
                ("wrong-commit", {"installer_overrides": {"commit": "b" * 40}}),
                ("wrong-target", {"installer_overrides": {"target": "aarch64-pc-windows-msvc"}}),
                ("wrong-nuspec-id", {"nuspec_overrides": {"id": "pr-sniper-localtest"}}),
                ("wrong-nuspec-version", {"nuspec_overrides": {"version": "0.1.1"}}),
                ("wrong-license-url", {"nuspec_overrides": {"licenseUrl": "https://example.invalid/LICENSE"}}),
                ("wrong-dependency", {"nuspec_overrides": {"dependency_version": "[1.0.0,)"}}),
                ("missing-unsigned-disclosure", {"nuspec_overrides": {"description": "Missing disclosure"}}),
                ("embedded-installer-exe", {"extra_entries": {"tools/installer.exe": b"embedded binary"}}),
                ("traversal-entry", {"extra_entries": {"../escape.txt": b"escape"}}),
                ("leading-slash-entry", {"extra_entries": {"/abs.txt": b"abs"}}),
                ("dot-segment-hook-entry", {"extra_entries": {"./tools/chocolateyBeforeModify.ps1": b"Write-Host 'hook'\n"}}),
                ("empty-segment-entry", {"extra_entries": {"tools//extra.txt": b"extra"}}),
                ("trailing-dot-segment", {"extra_entries": {"tools./chocolateyinstall.ps1": b"Write-Host 'dot'\n"}}),
                ("trailing-space-segment", {"extra_entries": {"tools /chocolateyinstall.ps1": b"Write-Host 'space'\n"}}),
                ("case-variant-duplicate-install-script", {"extra_entries": {"Tools/chocolateyinstall.ps1": b"Write-Host 'overwrite'\n"}}),
                ("case-variant-hook-script", {"extra_entries": {"Tools/chocolateyBeforeModify.ps1": b"Write-Host 'hook'\n"}}),
                ("outside-tools-hook-script", {"extra_entries": {"other/chocolateyBeforeModify.ps1": b"Write-Host 'hook'\n"}}),
                ("outside-tools-module-script", {"extra_entries": {"other/helper.psm1": b"Write-Host 'module'\n"}}),
                ("symlink-entry", {"extra_entries": {"tools/symlink.ps1": (symlink_info, b"target")}}),
                ("tampered-install-script", {"script_overrides": {"tools/chocolateyinstall.ps1": b"Write-Host 'tampered'\n"}}),
            ]:
                with self.subTest(case=label):
                    self.build_nupkg(nupkg, unsigned_manifest, **kwargs)
                    with self.assertRaises(release.ReleaseError):
                        release.verify_choco_nupkg(nupkg, unsigned_manifest, root=ROOT)

            self.build_nupkg(nupkg, signed_manifest, installer_overrides={"mode": "PublicUnsigned"})
            with self.assertRaises(release.ReleaseError):
                release.verify_choco_nupkg(nupkg, signed_manifest, root=ROOT)
            self.build_nupkg(nupkg, signed_manifest, installer_overrides={"signer_thumbprint": "B" * 40})
            with self.assertRaises(release.ReleaseError):
                release.verify_choco_nupkg(nupkg, signed_manifest, root=ROOT)

    def test_classify_choco_status_distinguishes_submitted_awaiting_moderation_rejected_and_approved(self):
        self.assertEqual(release.classify_choco_status(None, push_completed=False), "not_found")
        self.assertEqual(release.classify_choco_status(None, push_completed=True), "submitted")
        self.assertEqual(
            release.classify_choco_status({"IsApproved": False, "PackageStatus": "Pending"}),
            "awaiting_moderation",
        )
        self.assertEqual(
            release.classify_choco_status({"IsApproved": False, "SubmittedStatus": "Waiting"}),
            "awaiting_moderation",
        )
        self.assertEqual(
            release.classify_choco_status({"IsApproved": False, "PackageStatus": "Rejected"}),
            "rejected",
        )
        self.assertEqual(
            release.classify_choco_status({"IsApproved": True, "PackageStatus": "Approved"}),
            "publicly_approved",
        )
        for pending in [
            release.classify_choco_status(None, push_completed=True),
            release.classify_choco_status({"IsApproved": False, "PackageStatus": "Pending"}),
        ]:
            self.assertNotEqual(pending, "publicly_approved")

        class FeedOpener:
            def __init__(self, body=None, code=200):
                self.body = body
                self.code = code

            def open(self, request, timeout=90):
                self.accept = request.get_header("Accept")
                if self.code != 200:
                    raise release.urllib.error.HTTPError(request.full_url, self.code, "error", {}, None)
                response = unittest.mock.MagicMock()
                response.__enter__.return_value.read.return_value = self.body.encode("utf-8")
                return response

        missing = FeedOpener(code=404)
        self.assertEqual(
            release.choco_moderation_status("pr-sniper", "0.1.0", opener=missing),
            "not_found",
        )
        # community.chocolatey.org returns HTTP 406 when JSON is acceptable.
        self.assertEqual(missing.accept, "application/atom+xml, application/xml")
        self.assertNotIn("json", missing.accept)
        self.assertEqual(
            release.choco_moderation_status(
                "pr-sniper",
                "0.1.0",
                opener=FeedOpener(json.dumps({"d": {"IsApproved": False, "PackageStatus": "Pending"}})),
            ),
            "awaiting_moderation",
        )
        self.assertEqual(
            release.choco_moderation_status(
                "pr-sniper",
                "0.1.0",
                opener=FeedOpener(
                    '<entry xmlns:m="http://schemas.microsoft.com/ado/2007/08/dataservices/metadata" '
                    'xmlns:d="http://schemas.microsoft.com/ado/2007/08/dataservices">'
                    "<m:properties><d:IsApproved>true</d:IsApproved><d:PackageStatus>Approved</d:PackageStatus></m:properties>"
                    "</entry>"
                ),
            ),
            "publicly_approved",
        )

    def test_choco_publish_verifies_public_windows_bytes_handles_moderation_states_and_redacts_api_key(self):
        self.assets()
        release.publish(self.api, self.directory)
        with tempfile.TemporaryDirectory() as win_tmp:
            win_dir = Path(win_tmp)
            win_manifest = self.windows_assets(win_dir, trust_mode="unsigned")
            with patch.object(release, "command", return_value=COMMIT), \
                    patch.object(release, "config_version", return_value="0.1.0"), \
                    patch.object(self.api, "pages", side_effect=self.windows_pages):
                release.publish_windows(self.api, win_dir)

            self.api.corrupt_download = True
            with self.assertRaises(release.ReleaseError):
                release.choco_publish(self.api, win_dir, status_Reader=lambda *_: "not_found")
            self.api.corrupt_download = False

            self.build_nupkg(
                win_dir / "pr-sniper.0.1.0.nupkg",
                win_manifest,
                extra_entries={"tools/installer.exe": b"embedded"},
            )
            with patch.object(release, "command") as cmd, \
                    self.assertRaisesRegex(release.ReleaseError, "must not embed"):
                release.choco_publish(self.api, win_dir, status_Reader=lambda *_: "not_found")
            cmd.assert_not_called()
            self.build_nupkg(win_dir / "pr-sniper.0.1.0.nupkg", win_manifest)

            secret = "super-secret-choco-key-98765"
            with patch.dict(os.environ, {"CHOCOLATEY_API_KEY": secret}):
                completed = unittest.mock.Mock(
                    returncode=1, stdout="", stderr=f"push failed for key {secret}"
                )
                with patch.object(release.subprocess, "run", return_value=completed) as run_mock:
                    with self.assertRaises(release.ReleaseError) as ctx:
                        release.choco_publish(
                            self.api, win_dir, status_Reader=lambda *_: "not_found"
                        )
                    self.assertNotIn(secret, str(ctx.exception))
                    env_passed = run_mock.call_args.kwargs["env"]
                    self.assertNotIn("CHOCOLATEY_API_KEY", env_passed)

                pending_receipt = json.loads((win_dir / "choco-submission.json").read_text())
                self.assertEqual(pending_receipt["state"], "pending_push")
                self.assertIn("choco-intent.json", {a["name"] for a in self.api.assets})
                with patch.object(release, "command") as cmd, \
                        self.assertRaisesRegex(
                            release.ReleaseError,
                            "Prior Chocolatey submission intent is unconfirmed",
                        ):
                    release.choco_publish(
                        self.api, win_dir, status_Reader=lambda *_: "not_found"
                    )
                cmd.assert_not_called()

                with tempfile.TemporaryDirectory() as fresh_intent_tmp:
                    fresh_intent_dir = Path(fresh_intent_tmp)
                    self.windows_assets(fresh_intent_dir, trust_mode="unsigned")
                    with patch.object(release, "command") as cmd, \
                            self.assertRaisesRegex(
                                release.ReleaseError,
                                "Prior Chocolatey submission intent is unconfirmed",
                            ):
                        release.choco_publish(
                            self.api, fresh_intent_dir, status_Reader=lambda *_: "not_found"
                        )
                    cmd.assert_not_called()

                (win_dir / "choco-submission.json").unlink()
                (win_dir / "choco-intent.json").unlink()
                self.api.assets = [a for a in self.api.assets if a["name"] != "choco-intent.json"]

                original_request = self.api.request

                def fail_submission_upload(path, method="GET", value=None, missing=False, data=None, content_type=None):
                    if method == "POST" and path.endswith("name=choco-submission.json"):
                        raise release.ReleaseError("Simulated post-push receipt upload failure")
                    return original_request(path, method=method, value=value, missing=missing,
                                            data=data, content_type=content_type)

                with patch.object(self.api, "request", side_effect=fail_submission_upload), \
                        patch.object(release, "command", return_value="pushed") as cmd:
                    with self.assertRaisesRegex(release.ReleaseError, "Simulated post-push"):
                        release.choco_publish(
                            self.api, win_dir, status_Reader=lambda *_: "not_found"
                        )
                    cmd.assert_called_once()
                self.assertIn("choco-intent.json", {a["name"] for a in self.api.assets})
                self.assertNotIn("choco-submission.json", {a["name"] for a in self.api.assets})

                with tempfile.TemporaryDirectory() as fresh_after_drop_tmp:
                    fresh_drop_dir = Path(fresh_after_drop_tmp)
                    self.windows_assets(fresh_drop_dir, trust_mode="unsigned")
                    with patch.object(release, "command") as cmd, \
                            self.assertRaisesRegex(
                                release.ReleaseError,
                                "Prior Chocolatey submission intent is unconfirmed",
                            ):
                        release.choco_publish(
                            self.api, fresh_drop_dir, status_Reader=lambda *_: "not_found"
                        )
                    cmd.assert_not_called()

                (win_dir / "choco-submission.json").unlink()
                (win_dir / "choco-intent.json").unlink()
                self.api.assets = [a for a in self.api.assets if a["name"] != "choco-intent.json"]

                with patch.object(release, "command", return_value="pushed") as cmd:
                    result = release.choco_publish(
                        self.api, win_dir, status_Reader=lambda *_: "not_found"
                    )
                    self.assertEqual(result, {
                        "version": "0.1.0",
                        "source_sha": COMMIT,
                        "state": "submitted",
                    })
                    cmd.assert_called_once()

                submitted_receipt = json.loads((win_dir / "choco-submission.json").read_text())
                self.assertEqual(submitted_receipt["state"], "submitted")
                self.assertTrue({"choco-intent.json", "choco-submission.json"} <= {a["name"] for a in self.api.assets})

                macos_writes_before = len([r for r in self.api.requests if r[0] != "GET"])
                release.publish(self.api, self.directory)
                self.assertEqual(len([r for r in self.api.requests if r[0] != "GET"]), macos_writes_before)
                tap = Api()
                release.tap_update(self.api, tap, self.directory)
                release.tap_update(self.api, tap, self.directory)
                self.assertEqual(len([r for r in tap.requests if r[0] == "POST"]), 1)

                original_submission_bytes = self.api.bytes["choco-submission.json"]
                for bad_receipt in [
                    b"not-json",
                    json.dumps({**submitted_receipt, "state": "pending_push"}).encode("utf-8"),
                    json.dumps({**submitted_receipt, "source_sha": "b" * 40}).encode("utf-8"),
                ]:
                    self.api.bytes["choco-submission.json"] = bad_receipt
                    with self.assertRaises(release.ReleaseError):
                        release.publish(self.api, self.directory)
                    with self.assertRaises(release.ReleaseError):
                        release.tap_update(self.api, tap, self.directory)
                self.api.bytes["choco-submission.json"] = original_submission_bytes

                with patch.object(release, "command") as cmd:
                    rerun_same_dir = release.choco_publish(
                        self.api, win_dir, status_Reader=lambda *_: "not_found"
                    )
                    self.assertEqual(rerun_same_dir["state"], "submitted")
                    cmd.assert_not_called()

                with tempfile.TemporaryDirectory() as fresh_runner_tmp:
                    fresh_dir = Path(fresh_runner_tmp)
                    self.windows_assets(fresh_dir, trust_mode="unsigned")
                    self.assertFalse((fresh_dir / "choco-submission.json").exists())
                    with patch.object(release, "command") as cmd:
                        rerun_fresh_runner = release.choco_publish(
                            self.api, fresh_dir, status_Reader=lambda *_: "not_found"
                        )
                        self.assertEqual(rerun_fresh_runner["state"], "submitted")
                        cmd.assert_not_called()
                    self.assertEqual(
                        json.loads((fresh_dir / "choco-submission.json").read_text())["state"],
                        "submitted",
                    )

                    self.build_nupkg(
                        fresh_dir / "pr-sniper.0.1.0.nupkg",
                        win_manifest,
                        date_time=(2026, 1, 2, 0, 0, 0),
                    )
                    with patch.object(release, "command") as cmd, \
                            self.assertRaisesRegex(
                                release.ReleaseError,
                                "does not match local package bytes",
                            ):
                        release.choco_publish(
                            self.api, fresh_dir, status_Reader=lambda *_: "not_found"
                        )
                    cmd.assert_not_called()

            for live_state, expected_asset in (
                ("awaiting_moderation", "choco-awaiting-moderation.json"),
                ("publicly_approved", "choco-publicly-approved.json"),
            ):
                with patch.object(release, "command") as cmd:
                    result = release.choco_publish(
                        self.api, win_dir, status_Reader=lambda *_, s=live_state: s
                    )
                    self.assertEqual(result["state"], live_state)
                    self.assertEqual(result["version"], "0.1.0")
                    self.assertEqual(result["source_sha"], COMMIT)
                    self.assertEqual(
                        json.loads((win_dir / "choco-submission.json").read_text())["state"],
                        live_state,
                    )
                    self.assertIn(expected_asset, {a["name"] for a in self.api.assets})
                    cmd.assert_not_called()

                with tempfile.TemporaryDirectory() as fresh_transition_tmp:
                    fresh_transition_dir = Path(fresh_transition_tmp)
                    self.windows_assets(fresh_transition_dir, trust_mode="unsigned")

                    def fail_feed(*_):
                        raise release.ReleaseError("Temporary Chocolatey feed outage")

                    with patch.object(release, "command") as cmd:
                        recovered = release.choco_publish(
                            self.api, fresh_transition_dir, status_Reader=fail_feed
                        )
                        self.assertEqual(recovered["state"], live_state)
                        cmd.assert_not_called()

            with patch.object(release, "command") as cmd, \
                    self.assertRaisesRegex(release.ReleaseError, "Conflicting Chocolatey moderation receipts"):
                release.choco_publish(self.api, win_dir, status_Reader=lambda *_: "rejected")
            cmd.assert_not_called()

            self.api.assets = [a for a in self.api.assets if a["name"] != "choco-publicly-approved.json"]
            (win_dir / "choco-publicly-approved.json").unlink()
            (win_dir / "choco-submission.json").write_bytes((win_dir / "choco-awaiting-moderation.json").read_bytes())
            with patch.object(release, "command") as cmd, \
                    self.assertRaisesRegex(release.ReleaseError, "rejected in moderation"):
                release.choco_publish(self.api, win_dir, status_Reader=lambda *_: "rejected")
            cmd.assert_not_called()
            self.assertIn("choco-rejected.json", {a["name"] for a in self.api.assets})
            self.assertEqual(
                json.loads((win_dir / "choco-submission.json").read_text())["state"],
                "rejected",
            )
            with tempfile.TemporaryDirectory() as fresh_rejected_tmp:
                fresh_rejected_dir = Path(fresh_rejected_tmp)
                self.windows_assets(fresh_rejected_dir, trust_mode="unsigned")
                with patch.object(release, "command") as cmd, \
                        self.assertRaisesRegex(release.ReleaseError, "rejected in moderation"):
                    release.choco_publish(
                        self.api, fresh_rejected_dir, status_Reader=lambda *_: "not_found"
                    )
                cmd.assert_not_called()


if __name__ == "__main__":
    unittest.main()
