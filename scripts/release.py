#!/usr/bin/env python3
"""Tag-gated release metadata, immutable GitHub assets and Homebrew publication."""

import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
import zipfile

REPOSITORY = "jdylanmc/pr-sniper"
TAP = "jdylanmc/homebrew-pr-sniper"
BUNDLE_ID = "com.jdylanmc.pr-sniper"
WORKFLOW = ".github/workflows/macos.yml"
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
SHA = re.compile(r"[0-9a-f]{40}")
UPPER_SHA = re.compile(r"[0-9A-F]{40}")
HASH = re.compile(r"[0-9a-f]{64}")
ASSETS = Path(os.environ.get("RUNNER_TEMP", "/tmp")) / "pr-sniper-release-assets"
WINDOWS_ASSETS = Path(os.environ.get("RUNNER_TEMP", "/tmp")) / "pr-sniper-windows-release-assets"
CHOCO_PACKAGE_ID = "pr-sniper"
CHOCO_FEED = "https://community.chocolatey.org/api/v2"
CHOCO_PUSH_SOURCE = "https://push.chocolatey.org/"
CHOCO_STATES = ("publicly_approved", "awaiting_moderation", "rejected", "submitted", "not_found")
CHOCO_RECEIPT = "choco-submission.json"
CHOCO_STATE_ASSETS = {
    "pending_push": "choco-intent.json",
    "submitted": "choco-submission.json",
    "awaiting_moderation": "choco-awaiting-moderation.json",
    "rejected": "choco-rejected.json",
    "publicly_approved": "choco-publicly-approved.json",
}
CHOCO_ASSET_STATES = {name: state for state, name in CHOCO_STATE_ASSETS.items()}
CHOCO_RECEIPT_STATES = tuple(CHOCO_STATE_ASSETS)
CHOCO_WEBVIEW2_VERSION_RANGE = "[154.0.4258.62,)"


class ReleaseError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise ReleaseError(message)


def version(tag):
    require(isinstance(tag, str) and tag.startswith("v") and VERSION.fullmatch(tag[1:]),
            "Use a stable vMAJOR.MINOR.PATCH tag without leading zeroes.")
    return tag[1:]


def config_version(root=Path(".")):
    package = json.loads((root / "package.json").read_text())
    npm_lock = json.loads((root / "package-lock.json").read_text())
    config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
    cargo = (root / "src-tauri/Cargo.toml").read_text()
    cargo_lock = (root / "src-tauri/Cargo.lock").read_text()
    manifest = re.search(r'(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)', cargo)
    require(manifest, "Missing Cargo package.")
    rust_version = re.search(r'(?m)^version = "([^"]+)"$', manifest[1])
    locked = re.search(r'(?m)^name = "pr-sniper"\nversion = "([^"]+)"$', cargo_lock)
    require(rust_version and locked, "Missing Rust package version.")
    values = [package["version"], npm_lock["version"], npm_lock["packages"][""]["version"],
              config["version"], rust_version[1], locked[1]]
    require(len(set(values)) == 1 and VERSION.fullmatch(values[0]),
            "package.json, npm lock, Tauri config and Cargo manifest/lock must have one stable version.")
    require(config["identifier"] == BUNDLE_ID and config["productName"] == "PR Sniper",
            "Distribution must preserve PR Sniper's bundle and data identity.")
    require(config["bundle"]["macOS"]["minimumSystemVersion"] == "13.5",
            "Review cask compatibility before changing the minimum macOS version.")
    return values[0]


def command(args, timeout=120):
    try:
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("APPLE_")
                       and key not in ("GITHUB_TOKEN", "HOMEBREW_TAP_TOKEN", "CHOCOLATEY_API_KEY")}
        result = subprocess.run(args, check=False, capture_output=True, timeout=timeout, text=True, env=environment)
    except (OSError, subprocess.TimeoutExpired):
        raise ReleaseError("Command could not complete: " + str(args[0])) from None
    detail = ""
    if args[0] == "brew":
        detail = ": " + result.stderr[-3000:]
    elif args[0] == "choco":
        raw_detail = (result.stderr or result.stdout)[-3000:]
        for secret in [os.environ.get("CHOCOLATEY_API_KEY", ""),
                       *[args[index + 1] for index, arg in enumerate(args[:-1]) if arg == "--api-key"]]:
            if secret:
                raw_detail = raw_detail.replace(secret, "[REDACTED]")
        if raw_detail:
            detail = ": " + raw_detail
    require(result.returncode == 0, "Command failed: " + str(args[0]) + detail)
    return result.stdout.strip()


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Github:
    def __init__(self, token):
        require(token, "Required GitHub publishing credential is missing.")
        self.token = token

    def request(self, path, method="GET", value=None, missing=False, data=None,
                content_type="application/json"):
        require(path.startswith("/") and not path.startswith("//"), "Invalid GitHub API path.")
        host = "https://uploads.github.com" if data is not None else "https://api.github.com"
        if value is not None:
            data = json.dumps(value).encode()
        headers = {"Authorization": "Bearer " + self.token, "Accept": "application/vnd.github+json",
                   "X-GitHub-Api-Version": "2022-11-28", "Content-Type": content_type,
                   "User-Agent": "pr-sniper-release"}
        try:
            with urllib.request.build_opener(NoRedirect()).open(
                    urllib.request.Request(host + path, data=data, headers=headers, method=method),
                    timeout=90) as response:
                if response.status == 204 and method == "POST" and path.endswith("/dispatches"):
                    return None
                return json.load(response)
        except urllib.error.HTTPError as error:
            if error.code == 404 and missing:
                return None
            raise ReleaseError(f"GitHub {method} failed with HTTP {error.code}; no automatic mutation retry.") from None
        except (OSError, ValueError):
            raise ReleaseError(f"GitHub {method} response is unknown; inspect remote state before rerunning.") from None

    def pages(self, path, field=None):
        result = []
        for page in range(1, 101):
            value = self.request(path + ("&" if "?" in path else "?") + f"per_page=100&page={page}")
            rows = value[field] if field else value
            require(isinstance(rows, list), "Incomplete GitHub pagination response.")
            result.extend(rows)
            if len(rows) < 100:
                return result
        raise ReleaseError("GitHub pagination limit reached; no partial result accepted.")


def tag_sha(api, tag):
    version(tag)
    obj = api.request(f"/repos/{REPOSITORY}/git/ref/tags/{tag}")["object"]
    for _ in range(5):
        if obj["type"] == "commit":
            require(SHA.fullmatch(obj["sha"]), "Invalid tag commit.")
            return obj["sha"]
        require(obj["type"] == "tag", "Release tag must resolve to a commit.")
        obj = api.request(f"/repos/{REPOSITORY}/git/tags/{obj['sha']}")["object"]
    raise ReleaseError("Too many nested annotated tags.")


def gate(api, tag, root=Path("."), *, require_windows=False):
    expected = version(tag)
    require(config_version(root) == expected, "Tag does not match checked-in versions.")
    sha = command(["git", "rev-parse", "HEAD"])
    require(tag_sha(api, tag) == sha, "Checked-out source does not match the current release tag.")
    main = api.request(f"/repos/{REPOSITORY}/git/ref/heads/main")["object"]["sha"]
    comparison = api.request(f"/repos/{REPOSITORY}/compare/{sha}...{main}")
    require(comparison["status"] in ("ahead", "identical"), "Release source is not on main.")
    workflow = urllib.parse.quote(WORKFLOW, safe="")
    runs = api.pages(
        f"/repos/{REPOSITORY}/actions/workflows/{workflow}/runs?head_sha={sha}&event=push&branch=main",
        "workflow_runs")
    eligible = [run for run in runs if run.get("head_sha") == sha
                and run.get("head_branch") == "main" and run.get("event") == "push"]
    require(eligible, "No main-push CI exists for the exact tagged commit.")
    latest = max(eligible, key=lambda run: (run["id"], run.get("run_attempt", 1)))
    require(latest.get("status") == "completed" and latest.get("conclusion") == "success",
            "The latest exact-commit main CI must be complete and green.")
    jobs = api.pages(f"/repos/{REPOSITORY}/actions/runs/{latest['id']}/jobs", "jobs")
    require(any(job.get("name") == "macos" and job.get("conclusion") == "success" for job in jobs),
            "Required macOS CI job was not observed green.")
    if require_windows:
        windows_workflow = urllib.parse.quote(".github/workflows/windows.yml", safe="")
        windows_runs = api.pages(
            f"/repos/{REPOSITORY}/actions/workflows/{windows_workflow}/runs?head_sha={sha}&event=push&branch=main",
            "workflow_runs")
        eligible_windows = [run for run in windows_runs if run.get("head_sha") == sha
                            and run.get("head_branch") == "main" and run.get("event") == "push"]
        require(eligible_windows, "No exact-commit Windows main-push CI exists.")
        latest_windows = max(eligible_windows, key=lambda run: (run["id"], run.get("run_attempt", 1)))
        require(latest_windows.get("status") == "completed" and latest_windows.get("conclusion") == "success",
                "The latest exact-commit Windows main CI must be complete and green.")
        windows_jobs = api.pages(f"/repos/{REPOSITORY}/actions/runs/{latest_windows['id']}/jobs", "jobs")
        require(all(any(job.get("name") == name and job.get("conclusion") == "success"
                        for job in windows_jobs) for name in ("windows", "windows-installer-acceptance")),
                "Native Windows checks and installer acceptance must both be green.")
    require(tag_sha(api, tag) == sha, "Tag moved during eligibility checks.")
    newer_release_guard(api, expected)
    return {"sha": sha, "version": expected, "tag": tag}


def filename(release_version):
    version("v" + release_version)
    return f"pr-sniper-{release_version}-aarch64-apple-darwin.zip"


def windows_filename(release_version):
    return f"pr-sniper-{version('v' + release_version)}-x64-setup.exe"


def cask(release_version, checksum):
    filename(release_version)
    require(HASH.fullmatch(checksum), "Invalid asset checksum.")
    return f'''# frozen_string_literal: true

cask "pr-sniper" do
  version "{release_version}"
  sha256 "{checksum}"

  url "https://github.com/{REPOSITORY}/releases/download/v#{{version}}/pr-sniper-#{{version}}-aarch64-apple-darwin.zip"
  name "PR Sniper"
  desc "Human-owned pull request review from your menu bar"
  homepage "https://github.com/{REPOSITORY}"

  depends_on arch: :arm64
  depends_on macos: :ventura

  app "PR Sniper.app"

  caveats "Requires macOS 13.5 or later. Quit PR Sniper before upgrading; settings and credentials are preserved."
end
'''


def metadata(directory=ASSETS):
    manifest = json.loads((directory / "manifest.json").read_text())
    require(set(manifest) == {"version", "tag", "source_sha", "filename", "sha256", "architecture",
                              "bundle_id", "team_id", "notarization_id"}, "Unexpected release manifest.")
    require(version(manifest["tag"]) == manifest["version"] and SHA.fullmatch(manifest["source_sha"]),
            "Invalid release version or source.")
    require(manifest["source_sha"] == os.environ.get("SOURCE_SHA")
            and manifest["tag"] == os.environ.get("GITHUB_REF_NAME"),
            "Artifact provenance does not match this workflow's gated tag and commit.")
    require(manifest["filename"] == filename(manifest["version"])
            and manifest["architecture"] == "arm64" and manifest["bundle_id"] == BUNDLE_ID,
            "Wrong release platform, identity or filename.")
    require(re.fullmatch(r"[A-Z0-9]{10}", manifest["team_id"]), "Missing signer team.")
    require(re.fullmatch(r"[a-fA-F0-9-]{36}", manifest["notarization_id"]), "Missing notarization receipt.")
    artifact = directory / manifest["filename"]
    require(artifact.is_file() and not artifact.is_symlink(), "Release artifact is unavailable.")
    checksum = hashlib.sha256(artifact.read_bytes()).hexdigest()
    require(HASH.fullmatch(manifest["sha256"]) and manifest["sha256"] == checksum,
            "Release bytes do not match the signed artifact manifest.")
    return manifest


def _validate_windows_manifest_dict(manifest):
    required_keys = {"version", "tag", "source_sha", "filename", "sha256", "architecture", "bundle_id", "trust_mode"}
    allowed_keys = required_keys | {"signer_thumbprint", "signer_subject"}
    require(isinstance(manifest, dict) and required_keys <= set(manifest) <= allowed_keys,
            "Unexpected Windows release manifest.")
    require(isinstance(manifest["version"], str) and version(manifest["tag"]) == manifest["version"]
            and isinstance(manifest["source_sha"], str) and SHA.fullmatch(manifest["source_sha"]),
            "Invalid Windows release version or source.")
    if os.environ.get("SOURCE_SHA"):
        require(manifest["source_sha"] == os.environ["SOURCE_SHA"],
                "Windows artifact provenance does not match this workflow's gated commit.")
    if os.environ.get("GITHUB_REF_NAME"):
        require(manifest["tag"] == os.environ["GITHUB_REF_NAME"],
                "Windows artifact provenance does not match this workflow's gated tag.")
    require(manifest["filename"] == windows_filename(manifest["version"])
            and manifest["architecture"] == "x86_64" and manifest["bundle_id"] == BUNDLE_ID,
            "Wrong Windows release platform, identity or filename.")
    require(manifest["trust_mode"] in ("unsigned", "signed"), "Invalid Windows release trust mode.")
    if manifest["trust_mode"] == "unsigned":
        require(manifest.get("signer_thumbprint") is None and manifest.get("signer_subject") is None,
                "Unsigned Windows release manifest must not include signer identity.")
    else:
        thumbprint = manifest.get("signer_thumbprint")
        subject = manifest.get("signer_subject")
        require(isinstance(thumbprint, str) and UPPER_SHA.fullmatch(thumbprint)
                and isinstance(subject, str) and bool(subject.strip()),
                "Signed Windows release manifest requires uppercase 40-hex thumbprint and non-empty signer subject.")
    require(isinstance(manifest["sha256"], str) and HASH.fullmatch(manifest["sha256"]),
            "Invalid Windows installer checksum in manifest.")
    return manifest


def windows_metadata(directory=WINDOWS_ASSETS):
    manifest = _validate_windows_manifest_dict(json.loads((directory / "windows-manifest.json").read_text()))
    artifact = directory / manifest["filename"]
    require(artifact.is_file() and not artifact.is_symlink(), "Windows release artifact is unavailable.")
    checksum = hashlib.sha256(artifact.read_bytes()).hexdigest()
    require(manifest["sha256"] == checksum,
            "Windows release bytes do not match the artifact manifest.")
    sums = directory / "WINDOWS-SHA256SUMS"
    require(sums.is_file() and not sums.is_symlink(), "Windows checksum file is unavailable.")
    require(sums.read_bytes().decode() == f"{manifest['sha256']}  {manifest['filename']}\n",
            "Windows checksum file mismatch.")
    return manifest


def newer_release_guard(api, release_version):
    latest = api.request(f"/repos/{REPOSITORY}/releases/latest", missing=True)
    if latest is not None:
        latest_version = version(latest["tag_name"])
        require(tuple(map(int, release_version.split("."))) >= tuple(map(int, latest_version.split("."))),
                "Refusing to publish an older version as latest.")


def public_bytes(url):
    parsed = urllib.parse.urlparse(url)
    require(parsed.scheme == "https" and parsed.netloc == "github.com"
            and parsed.path.startswith(f"/{REPOSITORY}/releases/download/"),
            "Asset download is outside this product's releases.")
    # Public release downloads follow GitHub's CDN redirect without any credential.
    try:
        with urllib.request.urlopen(url, timeout=90) as response:
            require(response.url.startswith("https://"), "Insecure release asset redirect.")
            return response.read()
    except OSError:
        raise ReleaseError("Published release asset could not be downloaded.") from None


def release_for(api, manifest):
    release = api.request(f"/repos/{REPOSITORY}/releases/tags/{manifest['tag']}", missing=True)
    if release:
        require(release["tag_name"] == manifest["tag"] and not release["prerelease"]
                and release["target_commitish"] == manifest["source_sha"], "Existing release identity mismatch.")
    return release


def verify_public(api, manifest, local=None):
    require(tag_sha(api, manifest["tag"]) == manifest["source_sha"], "Tag moved; release is not current.")
    release = release_for(api, manifest)
    require(release and not release["draft"], "Release is not publicly published.")
    assets = api.pages(f"/repos/{REPOSITORY}/releases/{release['id']}/assets")
    expected = {manifest["filename"], "manifest.json", "SHA256SUMS"}
    windows_expected = {windows_filename(manifest["version"]), "windows-manifest.json", "WINDOWS-SHA256SUMS"}
    choco_allowed = set(CHOCO_STATE_ASSETS.values())
    names = {asset["name"] for asset in assets}
    require(len(assets) == len(names), "Release assets are incomplete, duplicated or unexpected.")
    require(
        expected <= names
        and (
            names == expected
            or (windows_expected <= names and (names - expected - windows_expected) <= choco_allowed)
        ),
        "Release assets are incomplete, duplicated or unexpected.",
    )
    base = f"https://github.com/{REPOSITORY}/releases/download/{manifest['tag']}/"
    for asset in assets:
        require(asset["state"] == "uploaded" and asset["browser_download_url"] == base + asset["name"],
                "Unexpected public release asset identity.")
        if asset["name"] not in expected:
            continue
        data = public_bytes(asset["browser_download_url"])
        if local is not None:
            require(data == (local / asset["name"]).read_bytes(), "Published bytes differ from local release assets.")
        if asset["name"] == manifest["filename"]:
            require(hashlib.sha256(data).hexdigest() == manifest["sha256"], "Published archive checksum mismatch.")
        elif asset["name"] == "manifest.json":
            require(json.loads(data) == manifest, "Published provenance manifest mismatch.")
        else:
            require(data.decode() == f"{manifest['sha256']}  {manifest['filename']}\n",
                    "Published checksum file mismatch.")
    if windows_expected <= names:
        win_asset = next(asset for asset in assets if asset["name"] == "windows-manifest.json")
        try:
            win_manifest = _validate_windows_manifest_dict(
                json.loads(public_bytes(win_asset["browser_download_url"]).decode("utf-8"))
            )
        except (UnicodeDecodeError, ValueError):
            raise ReleaseError("Published Windows provenance manifest is invalid JSON.") from None
        require(win_manifest["version"] == manifest["version"]
                and win_manifest["tag"] == manifest["tag"]
                and win_manifest["source_sha"] == manifest["source_sha"],
                "Published Windows provenance manifest does not match macOS release.")
        verify_windows_public(api, win_manifest)
    return release


def publish(api, directory=ASSETS):
    manifest = metadata(directory)
    require(tag_sha(api, manifest["tag"]) == manifest["source_sha"], "Tag moved before publication.")
    release = release_for(api, manifest)
    if release and not release["draft"]:
        verify_public(api, manifest, directory)
        return
    require(not release, "A draft from an interrupted release exists. Preserve it; reconcile manually before rerunning publication.")
    newer_release_guard(api, manifest["version"])
    release = api.request(f"/repos/{REPOSITORY}/releases", "POST", {
        "tag_name": manifest["tag"], "target_commitish": manifest["source_sha"],
        "name": "PR Sniper " + manifest["version"], "draft": True, "prerelease": False,
        "body": f"Developer ID signed and Apple-notarized macOS 13.5+ release for Apple Silicon.\n\n"
                f"Source: `{manifest['source_sha']}`. Homebrew cask is updated after asset verification.\n\n"
                "Quit PR Sniper before upgrading. Existing settings and credentials are preserved.",
    })
    for name in (manifest["filename"], "manifest.json", "SHA256SUMS"):
        data = (directory / name).read_bytes()
        result = api.request(f"/repos/{REPOSITORY}/releases/{release['id']}/assets?name={name}", "POST",
                             data=data, content_type="application/octet-stream")
        require(result["name"] == name and result["state"] == "uploaded" and result["size"] == len(data),
                "GitHub asset upload was not confirmed.")
        require(result.get("digest") == "sha256:" + hashlib.sha256(data).hexdigest(),
                "GitHub did not confirm the uploaded bytes' SHA-256 digest.")
    require(tag_sha(api, manifest["tag"]) == manifest["source_sha"], "Tag moved during upload.")
    api.request(f"/repos/{REPOSITORY}/releases/{release['id']}", "PATCH", {"draft": False, "make_latest": "true"})
    verify_public(api, manifest, directory)


def choco_receipt_payload(manifest, nupkg_sha256, state):
    require(state in CHOCO_RECEIPT_STATES, "Invalid Chocolatey submission receipt state.")
    return {
        "package_id": CHOCO_PACKAGE_ID,
        "version": manifest["version"],
        "tag": manifest["tag"],
        "source_sha": manifest["source_sha"],
        "installer_sha256": manifest["sha256"],
        "nupkg_sha256": nupkg_sha256,
        "state": state,
    }


def validate_choco_receipt(receipt, manifest, nupkg_sha256=None, expected_state=None):
    expected_keys = {
        "package_id", "version", "tag", "source_sha",
        "installer_sha256", "nupkg_sha256", "state",
    }
    require(isinstance(receipt, dict) and set(receipt) == expected_keys,
            "Unexpected Chocolatey submission receipt schema.")
    require(receipt["package_id"] == CHOCO_PACKAGE_ID
            and receipt["version"] == manifest["version"]
            and receipt["tag"] == manifest["tag"]
            and receipt["source_sha"] == manifest["source_sha"]
            and receipt["installer_sha256"] == manifest["sha256"],
            "Chocolatey submission receipt does not match release provenance.")
    require(isinstance(receipt["nupkg_sha256"], str) and HASH.fullmatch(receipt["nupkg_sha256"]),
            "Invalid Chocolatey package checksum in submission receipt.")
    if nupkg_sha256 is not None:
        require(receipt["nupkg_sha256"] == nupkg_sha256,
                "Chocolatey submission receipt does not match local package bytes.")
    require(receipt["state"] in CHOCO_RECEIPT_STATES,
            "Invalid Chocolatey submission receipt state.")
    if expected_state is not None:
        require(receipt["state"] == expected_state,
                "Chocolatey submission receipt state does not match asset identity.")
    return receipt


def _write_receipt_file(receipt_path, raw):
    require(not receipt_path.is_symlink(), "Chocolatey submission receipt must not be a symlink.")
    temp_path = receipt_path.parent / f".{receipt_path.name}.tmp"
    require(not temp_path.is_symlink(), "Temporary Chocolatey submission receipt must not be a symlink.")
    temp_path.write_bytes(raw)
    temp_path.replace(receipt_path)


def write_choco_receipt(receipt_path, manifest, nupkg_sha256, state):
    payload = choco_receipt_payload(manifest, nupkg_sha256, state)
    raw = (json.dumps(payload) + "\n").encode("utf-8")
    _write_receipt_file(receipt_path, raw)
    return raw


def _upload_choco_receipt(api, release_id, asset_name, receipt, existing_names, directory=None):
    require(asset_name in CHOCO_ASSET_STATES, "Unexpected Chocolatey receipt asset name.")
    require(receipt.get("state") == CHOCO_ASSET_STATES[asset_name],
            "Chocolatey receipt state does not match target asset name.")
    raw = (json.dumps(receipt) + "\n").encode("utf-8")
    if directory is not None:
        _write_receipt_file(Path(directory) / CHOCO_RECEIPT, raw)
        if asset_name != CHOCO_RECEIPT:
            _write_receipt_file(Path(directory) / asset_name, raw)
    if asset_name not in existing_names:
        result = api.request(
            f"/repos/{REPOSITORY}/releases/{release_id}/assets?name={asset_name}",
            "POST",
            data=raw,
            content_type="application/octet-stream",
        )
        require(result["name"] == asset_name and result["state"] == "uploaded"
                and result["size"] == len(raw),
                "GitHub Chocolatey submission receipt upload was not confirmed.")
        require(result.get("digest") == "sha256:" + hashlib.sha256(raw).hexdigest(),
                "GitHub did not confirm the Chocolatey submission receipt SHA-256 digest.")
        existing_names.add(asset_name)
    return raw


def _reconcile_choco_receipts(api, manifest, directory, nupkg_sha256, assets):
    states = set()
    for asset in assets:
        name = asset["name"]
        if name not in CHOCO_ASSET_STATES:
            continue
        remote_bytes = public_bytes(asset["browser_download_url"])
        try:
            receipt = json.loads(remote_bytes.decode("utf-8"))
        except (UnicodeDecodeError, ValueError):
            raise ReleaseError("Published Chocolatey submission receipt is invalid JSON.") from None
        validate_choco_receipt(receipt, manifest, nupkg_sha256, expected_state=CHOCO_ASSET_STATES[name])
        states.add(receipt["state"])

    if directory is not None:
        dir_path = Path(directory)
        local_candidates = [(dir_path / CHOCO_RECEIPT, None)] + [
            (dir_path / asset_name, state_key)
            for state_key, asset_name in CHOCO_STATE_ASSETS.items()
            if asset_name != CHOCO_RECEIPT
        ]
        for path, expected_state in local_candidates:
            if not path.exists() and not path.is_symlink():
                continue
            require(path.is_file() and not path.is_symlink(),
                    "Invalid local Chocolatey submission receipt file.")
            try:
                receipt = json.loads(path.read_text(encoding="utf-8"))
            except ValueError:
                raise ReleaseError("Local Chocolatey submission receipt is invalid JSON.") from None
            validate_choco_receipt(receipt, manifest, nupkg_sha256, expected_state=expected_state)
            states.add(receipt["state"])

    require(not ({"rejected", "publicly_approved"} <= states),
            "Conflicting Chocolatey moderation receipts.")
    for candidate in ("rejected", "publicly_approved", "awaiting_moderation", "submitted", "pending_push"):
        if candidate in states:
            return candidate
    return None


def verify_windows_public(api, manifest, local=None):
    require(tag_sha(api, manifest["tag"]) == manifest["source_sha"], "Tag moved; release is not current.")
    release = release_for(api, manifest)
    require(release and not release["draft"], "Release is not publicly published.")
    assets = api.pages(f"/repos/{REPOSITORY}/releases/{release['id']}/assets")
    windows_expected = {manifest["filename"], "windows-manifest.json", "WINDOWS-SHA256SUMS"}
    macos_expected = {filename(manifest["version"]), "manifest.json", "SHA256SUMS"}
    required_names = windows_expected | macos_expected
    allowed_names = required_names | set(CHOCO_STATE_ASSETS.values())
    names = {asset["name"] for asset in assets}
    require(len(assets) == len(names), "Release assets are duplicated.")
    require(required_names <= names <= allowed_names,
            "Release assets are incomplete, duplicated or unexpected.")
    base = f"https://github.com/{REPOSITORY}/releases/download/{manifest['tag']}/"
    seen_choco_states = set()
    for asset in assets:
        require(asset["state"] == "uploaded" and asset["browser_download_url"] == base + asset["name"],
                "Unexpected public release asset identity.")
        if asset["name"] in CHOCO_ASSET_STATES:
            data = public_bytes(asset["browser_download_url"])
            try:
                receipt = json.loads(data.decode("utf-8"))
            except (UnicodeDecodeError, ValueError):
                raise ReleaseError("Published Chocolatey submission receipt is invalid JSON.") from None
            validate_choco_receipt(receipt, manifest, expected_state=CHOCO_ASSET_STATES[asset["name"]])
            seen_choco_states.add(receipt["state"])
            continue
        if asset["name"] not in windows_expected:
            continue
        data = public_bytes(asset["browser_download_url"])
        if local is not None:
            require(data == (local / asset["name"]).read_bytes(),
                    "Published Windows bytes differ from local release assets.")
        if asset["name"] == manifest["filename"]:
            require(hashlib.sha256(data).hexdigest() == manifest["sha256"],
                    "Published Windows installer checksum mismatch.")
        elif asset["name"] == "windows-manifest.json":
            require(json.loads(data) == manifest, "Published Windows provenance manifest mismatch.")
        else:
            require(data.decode() == f"{manifest['sha256']}  {manifest['filename']}\n",
                    "Published Windows checksum file mismatch.")
    require(not ({"rejected", "publicly_approved"} <= seen_choco_states),
            "Conflicting Chocolatey moderation receipts.")
    return release


def publish_windows(api, directory=WINDOWS_ASSETS):
    manifest = windows_metadata(directory)
    gated = gate(api, manifest["tag"], require_windows=True)
    require(gated["sha"] == manifest["source_sha"], "Windows release source does not match gated commit.")
    release = release_for(api, manifest)
    require(release and not release["draft"],
            "Published GitHub release must exist before attaching Windows assets.")
    assets = api.pages(f"/repos/{REPOSITORY}/releases/{release['id']}/assets")
    windows_names = (manifest["filename"], "windows-manifest.json", "WINDOWS-SHA256SUMS")
    existing_names = {asset["name"] for asset in assets}
    present_windows = existing_names & set(windows_names)
    if present_windows == set(windows_names):
        return verify_windows_public(api, manifest, directory)
    require(not present_windows,
            "Partial Windows release assets exist; reconcile manually before rerunning.")
    macos_expected = {filename(manifest["version"]), "manifest.json", "SHA256SUMS"}
    require(existing_names == macos_expected,
            "Existing release assets are incomplete or unexpected.")
    for name in windows_names:
        data = (directory / name).read_bytes()
        result = api.request(f"/repos/{REPOSITORY}/releases/{release['id']}/assets?name={name}", "POST",
                             data=data, content_type="application/octet-stream")
        require(result["name"] == name and result["state"] == "uploaded" and result["size"] == len(data),
                "GitHub Windows asset upload was not confirmed.")
        require(result.get("digest") == "sha256:" + hashlib.sha256(data).hexdigest(),
                "GitHub did not confirm the uploaded Windows bytes' SHA-256 digest.")
    require(tag_sha(api, manifest["tag"]) == manifest["source_sha"], "Tag moved during Windows upload.")
    return verify_windows_public(api, manifest, directory)


def verify_choco_nupkg(package_file, manifest, root=Path(".")):
    package_file = Path(package_file)
    require(package_file.is_file() and not package_file.is_symlink(),
            "Chocolatey package file is unavailable.")
    require(package_file.name == f"{CHOCO_PACKAGE_ID}.{manifest['version']}.nupkg"
            and "localtest" not in package_file.name.lower(),
            "Refusing to publish non-public or mismatched Chocolatey package.")
    repo_root = Path(root)
    if repo_root == Path(".") and not (repo_root / "packaging" / "chocolatey").is_dir():
        repo_root = Path(__file__).resolve().parents[1]
    try:
        with zipfile.ZipFile(package_file, "r") as archive:
            entries = {}
            for info in archive.infolist():
                raw_name = info.filename
                norm_name = raw_name.replace("\\", "/")
                require(not raw_name.startswith(("/", "\\")) and ":" not in raw_name
                        and ".." not in norm_name.split("/"),
                        "Unsafe path in Chocolatey package archive.")
                require(((info.external_attr >> 16) & 0o170000) != 0o120000
                        and not (info.external_attr & 0x400),
                        "Symlink entry is not permitted in Chocolatey package archive.")
                if info.is_dir() or norm_name.endswith("/"):
                    continue
                require(norm_name not in entries, "Duplicate entry in Chocolatey package archive.")
                require(norm_name.lower() != "tools/installer.exe"
                        and not norm_name.lower().endswith((".exe", ".dll", ".msi", ".bat", ".cmd")),
                        "Public Chocolatey package must not embed an installer executable.")
                entries[norm_name] = info

            require("pr-sniper.nuspec" in entries, "Chocolatey package is missing pr-sniper.nuspec.")
            try:
                nuspec_root = ET.fromstring(archive.read(entries["pr-sniper.nuspec"]).decode("utf-8-sig"))
            except (UnicodeDecodeError, ET.ParseError):
                raise ReleaseError("Chocolatey nuspec is invalid XML.") from None
            require(nuspec_root.tag.rsplit("}", 1)[-1] == "package", "Unexpected Chocolatey nuspec root.")
            meta_elem = next(
                (child for child in nuspec_root if child.tag.rsplit("}", 1)[-1] == "metadata"),
                None,
            )
            require(meta_elem is not None, "Chocolatey nuspec is missing metadata.")
            meta_text = {
                child.tag.rsplit("}", 1)[-1]: (child.text or "").strip()
                for child in meta_elem
            }
            expected_license = f"https://github.com/{REPOSITORY}/blob/{manifest['source_sha']}/LICENSE"
            require(meta_text.get("id") == CHOCO_PACKAGE_ID
                    and meta_text.get("version") == manifest["version"]
                    and meta_text.get("licenseUrl") == expected_license,
                    "Chocolatey nuspec metadata does not match release provenance.")
            deps_elem = next(
                (child for child in meta_elem if child.tag.rsplit("}", 1)[-1] == "dependencies"),
                None,
            )
            require(deps_elem is not None, "Chocolatey nuspec is missing dependencies.")
            deps = [
                (elem.attrib.get("id"), elem.attrib.get("version"))
                for elem in deps_elem.iter()
                if elem.tag.rsplit("}", 1)[-1] == "dependency"
            ]
            require(deps == [("webview2-runtime", CHOCO_WEBVIEW2_VERSION_RANGE)],
                    "Chocolatey nuspec is missing required webview2-runtime dependency.")
            description = meta_text.get("description", "")
            require(bool(description) and "NEVER PUBLISH" not in description,
                    "Invalid Chocolatey nuspec description.")
            if manifest["trust_mode"] == "unsigned":
                require("UNSIGNED PUBLIC RELEASE" in description,
                        "Unsigned public Chocolatey nuspec must disclose unsigned status.")
            else:
                require("UNSIGNED PUBLIC RELEASE" not in description,
                        "Signed public Chocolatey nuspec must not claim unsigned status.")

            require("tools/installer.json" in entries, "Chocolatey package is missing tools/installer.json.")
            try:
                installer_meta = json.loads(archive.read(entries["tools/installer.json"]).decode("utf-8-sig"))
            except (UnicodeDecodeError, ValueError):
                raise ReleaseError("Chocolatey installer.json is invalid JSON.") from None
            require(isinstance(installer_meta, dict), "Invalid Chocolatey installer.json.")
            expected_url = f"https://github.com/{REPOSITORY}/releases/download/{manifest['tag']}/{manifest['filename']}"
            require(installer_meta.get("package_id") == CHOCO_PACKAGE_ID
                    and installer_meta.get("version") == manifest["version"]
                    and installer_meta.get("commit") == manifest["source_sha"]
                    and installer_meta.get("target") == "x86_64-pc-windows-msvc"
                    and installer_meta.get("sha256") == manifest["sha256"]
                    and installer_meta.get("url") == expected_url,
                    "Chocolatey installer.json does not match release manifest.")
            if manifest["trust_mode"] == "unsigned":
                require(installer_meta.get("mode") == "PublicUnsigned"
                        and not installer_meta.get("signer_thumbprint")
                        and not installer_meta.get("signer_subject"),
                        "Unsigned Chocolatey installer.json must use PublicUnsigned mode without signer identity.")
            else:
                require(installer_meta.get("mode") == "Public"
                        and installer_meta.get("signer_thumbprint") == manifest["signer_thumbprint"]
                        and installer_meta.get("signer_subject") == manifest["signer_subject"],
                        "Signed Chocolatey installer.json must use Public mode with matching signer identity.")

            expected_scripts = {
                "tools/chocolateyinstall.ps1": repo_root / "packaging" / "chocolatey" / "chocolateyinstall.ps1",
                "tools/chocolateyuninstall.ps1": repo_root / "packaging" / "chocolatey" / "chocolateyuninstall.ps1",
                "tools/removal-state.ps1": repo_root / "packaging" / "chocolatey" / "removal-state.ps1",
                "tools/windows-signature.ps1": repo_root / "scripts" / "windows-signature.ps1",
                "tools/windows-webview2.ps1": repo_root / "scripts" / "windows-webview2.ps1",
            }
            allowed_tools = set(expected_scripts) | {"tools/installer.json", "tools/installer.exe.ignore"}
            tool_entries = {name for name in entries if name.startswith("tools/")}
            require(set(expected_scripts) <= tool_entries <= allowed_tools,
                    "Chocolatey package tools/ directory has missing or unexpected files.")
            for entry_name, source_path in expected_scripts.items():
                require(source_path.is_file() and not source_path.is_symlink(),
                        f"Checked-in Chocolatey script is unavailable: {source_path}")
                packaged_bytes = archive.read(entries[entry_name]).replace(b"\r\n", b"\n")
                checked_in_bytes = source_path.read_bytes().replace(b"\r\n", b"\n")
                require(hashlib.sha256(packaged_bytes).hexdigest() == hashlib.sha256(checked_in_bytes).hexdigest(),
                        f"Packaged Chocolatey script does not match checked-in source: {entry_name}")
    except zipfile.BadZipFile:
        raise ReleaseError("Invalid Chocolatey package archive.") from None
    return hashlib.sha256(package_file.read_bytes()).hexdigest()


def classify_choco_status(feed_entry, push_completed=False):
    if feed_entry is None:
        return "submitted" if push_completed else "not_found"
    if isinstance(feed_entry, str):
        normalized = feed_entry.strip().lower()
        if normalized in CHOCO_STATES:
            return normalized
        raise ReleaseError("Unrecognized Chocolatey moderation status.")
    require(isinstance(feed_entry, dict) and bool(feed_entry),
            "Unrecognized Chocolatey moderation status.")
    explicit = str(feed_entry.get("state") or feed_entry.get("status")
                   or feed_entry.get("moderation_status") or "").strip().lower()
    if explicit in CHOCO_STATES:
        return explicit
    package_status = str(feed_entry.get("PackageStatus") or feed_entry.get("package_status") or "").strip().lower()
    submitted_status = str(feed_entry.get("SubmittedStatus") or feed_entry.get("submitted_status") or "").strip().lower()
    raw_approved = feed_entry.get("IsApproved", feed_entry.get("is_approved"))
    if isinstance(raw_approved, str):
        approved = raw_approved.strip().lower() == "true"
    elif isinstance(raw_approved, bool):
        approved = raw_approved
    else:
        approved = None
    if "reject" in package_status or "reject" in submitted_status or explicit == "rejected":
        return "rejected"
    if approved is True and package_status in ("", "approved", "active", "published", "exempted"):
        return "publicly_approved"
    if approved is False or package_status in ("pending", "waiting", "submitted", "validating", "verifying", "responded", "ready", "pendingmoderation", "awaitingmoderation") \
            or submitted_status in ("pending", "waiting", "submitted", "validating", "verifying", "responded", "ready"):
        return "awaiting_moderation"
    raise ReleaseError("Unrecognized Chocolatey moderation status.")


def choco_moderation_status(package_id=CHOCO_PACKAGE_ID, package_version="", *, opener=None):
    require(package_id == CHOCO_PACKAGE_ID, "Unexpected Chocolatey package ID.")
    version("v" + package_version)
    url = f"{CHOCO_FEED}/Packages(Id='{package_id}',Version='{package_version}')"
    client = opener if opener is not None else urllib.request.build_opener(NoRedirect())
    request = urllib.request.Request(url, headers={
        "Accept": "application/json, application/atom+xml, application/xml;q=0.9",
        "User-Agent": "pr-sniper-release",
    })
    try:
        with client.open(request, timeout=90) as response:
            raw = response.read().decode("utf-8")
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return "not_found"
        raise ReleaseError(f"Chocolatey feed query failed with HTTP {error.code}.") from None
    except OSError:
        raise ReleaseError("Chocolatey feed query could not complete.") from None
    stripped = raw.strip()
    require(stripped, "Empty Chocolatey feed response.")
    if stripped.startswith("{"):
        payload = json.loads(stripped)
        entry = payload.get("d", payload) if isinstance(payload, dict) else None
        require(isinstance(entry, dict), "Unexpected Chocolatey JSON feed entry.")
        return classify_choco_status(entry)
    root = ET.fromstring(stripped)
    props = {}
    for elem in root.iter():
        local_name = elem.tag.rsplit("}", 1)[-1]
        if local_name in ("IsApproved", "PackageStatus", "SubmittedStatus", "Id", "Version") and elem.text is not None:
            props[local_name] = elem.text.strip()
    if not props and root.tag.rsplit("}", 1)[-1] == "feed":
        return "not_found"
    return classify_choco_status(props)


def choco_publish(api, directory=WINDOWS_ASSETS, nupkg_path=None, status_reader=None, status_Reader=None, root=Path(".")):
    manifest = windows_metadata(directory)
    release = verify_windows_public(api, manifest, directory)
    package_file = Path(nupkg_path) if nupkg_path is not None else (directory / f"{CHOCO_PACKAGE_ID}.{manifest['version']}.nupkg")
    nupkg_sha256 = verify_choco_nupkg(package_file, manifest, root=root)
    assets = api.pages(f"/repos/{REPOSITORY}/releases/{release['id']}/assets")
    existing_names = {asset["name"] for asset in assets}
    durable_state = _reconcile_choco_receipts(api, manifest, directory, nupkg_sha256, assets)

    reader = status_reader or status_Reader or choco_moderation_status
    try:
        raw_status = reader(CHOCO_PACKAGE_ID, manifest["version"])
        current_status = classify_choco_status(raw_status) if isinstance(raw_status, dict) else raw_status
        require(current_status in CHOCO_STATES, "Unknown Chocolatey moderation state.")
    except ReleaseError:
        if durable_state in ("rejected", "publicly_approved", "awaiting_moderation", "submitted"):
            current_status = "not_found"
        else:
            raise

    require(not ({"rejected", "publicly_approved"} <= {current_status, durable_state}),
            "Conflicting Chocolatey moderation receipts.")
    if current_status == "rejected" or durable_state == "rejected":
        receipt = choco_receipt_payload(manifest, nupkg_sha256, "rejected")
        _upload_choco_receipt(
            api, release["id"], CHOCO_STATE_ASSETS["rejected"], receipt, existing_names, directory
        )
        raise ReleaseError("Chocolatey package version was rejected in moderation; do not push duplicate version.")
    if current_status in ("publicly_approved", "awaiting_moderation", "submitted"):
        receipt = choco_receipt_payload(manifest, nupkg_sha256, current_status)
        _upload_choco_receipt(
            api, release["id"], CHOCO_STATE_ASSETS[current_status], receipt, existing_names, directory
        )
        return {"version": manifest["version"], "source_sha": manifest["source_sha"], "state": current_status}
    if durable_state in ("publicly_approved", "awaiting_moderation", "submitted"):
        receipt = choco_receipt_payload(manifest, nupkg_sha256, durable_state)
        _upload_choco_receipt(
            api, release["id"], CHOCO_STATE_ASSETS[durable_state], receipt, existing_names, directory
        )
        return {"version": manifest["version"], "source_sha": manifest["source_sha"], "state": durable_state}
    if durable_state == "pending_push":
        receipt = choco_receipt_payload(manifest, nupkg_sha256, "pending_push")
        _upload_choco_receipt(
            api, release["id"], CHOCO_STATE_ASSETS["pending_push"], receipt, existing_names, directory
        )
        raise ReleaseError("Prior Chocolatey submission intent is unconfirmed; reconcile moderation state before retrying.")

    api_key = os.environ.get("CHOCOLATEY_API_KEY", "").strip()
    require(api_key, "Required Chocolatey publishing credential is missing.")
    require(tag_sha(api, manifest["tag"]) == manifest["source_sha"], "Tag moved before Chocolatey publication.")
    intent_receipt = choco_receipt_payload(manifest, nupkg_sha256, "pending_push")
    _upload_choco_receipt(
        api, release["id"], CHOCO_STATE_ASSETS["pending_push"], intent_receipt, existing_names, directory
    )
    try:
        command(["choco", "push", str(package_file), "--source", CHOCO_PUSH_SOURCE, "--api-key", api_key])
    except ReleaseError as error:
        raise ReleaseError(str(error).replace(api_key, "[REDACTED]")) from None
    submitted_state = classify_choco_status(None, push_completed=True)
    submitted_receipt = choco_receipt_payload(manifest, nupkg_sha256, submitted_state)
    _upload_choco_receipt(
        api, release["id"], CHOCO_STATE_ASSETS[submitted_state], submitted_receipt, existing_names, directory
    )
    return {
        "version": manifest["version"],
        "source_sha": manifest["source_sha"],
        "state": submitted_state,
    }


def tap_ready(api):
    repository = api.request(f"/repos/{TAP}")
    require(repository["full_name"] == TAP and repository["default_branch"] == "main"
            and not repository["private"] and repository.get("permissions", {}).get("push") is True,
            "The tap credential must have write access to the exact public tap's main branch.")
    workflow = api.request(f"/repos/{TAP}/contents/.github/workflows/publish.yml?ref=main")
    require(workflow.get("type") == "file", "Merge the tap-owned publisher before releasing.")


def tap_update(api, tap_api, directory=ASSETS):
    manifest = metadata(directory)
    verify_public(api, manifest, directory)
    path = f"/repos/{TAP}/contents/Casks/pr-sniper.rb"
    existing = tap_api.request(path + "?ref=main", missing=True)
    content = cask(manifest["version"], manifest["sha256"])
    if existing:
        require(existing.get("type") == "file" and existing.get("encoding") == "base64", "Unexpected cask file.")
        old = base64.b64decode(existing["content"]).decode()
        if old == content:
            return
        found = re.search(r'(?m)^  version "([^"]+)"$', old)
        require(found and VERSION.fullmatch(found[1]), "Existing cask version cannot be compared.")
        old_version = tuple(map(int, found[1].split(".")))
        new_version = tuple(map(int, manifest["version"].split(".")))
        require(new_version > old_version, "Refusing a same-version checksum change or tap downgrade.")
    require(tag_sha(api, manifest["tag"]) == manifest["source_sha"], "Tag moved before tap update.")
    tap_api.request(f"/repos/{TAP}/dispatches", "POST", {
        "event_type": "pr-sniper-release",
        "client_payload": {"tag": manifest["tag"], "source_sha": manifest["source_sha"],
                           "sha256": manifest["sha256"]},
    })
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        confirmed = tap_api.request(path + "?ref=main", missing=True)
        if confirmed is not None:
            require(confirmed.get("type") == "file" and confirmed.get("encoding") == "base64",
                    "Unexpected tap confirmation response.")
            if base64.b64decode(confirmed["content"]).decode() == content:
                require(tag_sha(api, manifest["tag"]) == manifest["source_sha"],
                        "Tag moved before tap confirmation.")
                return
        time.sleep(20)
    raise ReleaseError("Tap dispatch was accepted, but matching cask publication is unconfirmed. Inspect the tap publisher; no automatic redispatch.")


def main():
    action = sys.argv[1] if len(sys.argv) == 2 else ""
    if action == "check":
        print("Release version:", config_version())
        return
    require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY
            and os.environ.get("GITHUB_REF_TYPE") == "tag", "Release effects require this repository's tag workflow.")
    tag = os.environ.get("GITHUB_REF_NAME", "")
    version(tag)
    api = Github(os.environ.get("GITHUB_TOKEN"))
    if action == "gate":
        result = gate(api, tag)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write("sha=" + result["sha"] + "\nversion=" + result["version"] + "\n")
    elif action == "check-windows-release":
        print(json.dumps(gate(api, tag, require_windows=True)))
    elif action == "tap-ready":
        tap_ready(Github(os.environ.get("HOMEBREW_TAP_TOKEN")))
    elif action == "publish":
        publish(api)
    elif action == "publish-windows":
        publish_windows(api)
    elif action == "tap":
        tap_update(api, Github(os.environ.get("HOMEBREW_TAP_TOKEN")))
    elif action == "choco-publish":
        print(json.dumps(choco_publish(api)))
    else:
        raise ReleaseError("Expected check, gate, check-windows-release, tap-ready, publish, publish-windows, tap or choco-publish.")


if __name__ == "__main__":
    try:
        main()
    except (ReleaseError, KeyError, ValueError, OSError) as error:
        print("Release failed: " + (str(error) if isinstance(error, ReleaseError) else "Invalid or unavailable release data."), file=sys.stderr)
        sys.exit(1)
