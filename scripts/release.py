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

REPOSITORY = "jdylanmc/pr-sniper"
TAP = "jdylanmc/homebrew-pr-sniper"
BUNDLE_ID = "com.jdylanmc.pr-sniper"
WORKFLOW = ".github/workflows/macos.yml"
VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
SHA = re.compile(r"[0-9a-f]{40}")
HASH = re.compile(r"[0-9a-f]{64}")
ASSETS = Path(os.environ.get("RUNNER_TEMP", "/tmp")) / "pr-sniper-release-assets"


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
                       if not key.startswith("APPLE_") and key not in ("GITHUB_TOKEN", "HOMEBREW_TAP_TOKEN")}
        result = subprocess.run(args, check=False, capture_output=True, timeout=timeout, text=True, env=environment)
    except (OSError, subprocess.TimeoutExpired):
        raise ReleaseError("Command could not complete: " + str(args[0])) from None
    require(result.returncode == 0, "Command failed: " + str(args[0])
            + (": " + result.stderr[-3000:] if args[0] == "brew" else ""))
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


def gate(api, tag, root=Path(".")):
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
    require(tag_sha(api, tag) == sha, "Tag moved during eligibility checks.")
    newer_release_guard(api, expected)
    return {"sha": sha, "version": expected, "tag": tag}


def filename(release_version):
    version("v" + release_version)
    return f"pr-sniper-{release_version}-aarch64-apple-darwin.zip"


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
    require({asset["name"] for asset in assets} == expected and len(assets) == len(expected),
            "Release assets are incomplete, duplicated or unexpected.")
    base = f"https://github.com/{REPOSITORY}/releases/download/{manifest['tag']}/"
    for asset in assets:
        require(asset["state"] == "uploaded" and asset["browser_download_url"] == base + asset["name"],
                "Unexpected public release asset identity.")
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
    elif action == "tap-ready":
        tap_ready(Github(os.environ.get("HOMEBREW_TAP_TOKEN")))
    elif action == "publish":
        publish(api)
    elif action == "tap":
        tap_update(api, Github(os.environ.get("HOMEBREW_TAP_TOKEN")))
    else:
        raise ReleaseError("Expected check, gate, tap-ready, publish or tap.")


if __name__ == "__main__":
    try:
        main()
    except (ReleaseError, KeyError, ValueError, OSError) as error:
        print("Release failed: " + (str(error) if isinstance(error, ReleaseError) else "Invalid or unavailable release data."), file=sys.stderr)
        sys.exit(1)
