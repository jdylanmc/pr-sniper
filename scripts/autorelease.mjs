#!/usr/bin/env node
// Autonomous release stages (#296): plan → land → verify → tag → release.
// Each stage detects finished work, so re-dispatching a version resumes it.
import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  RELEASE_BRANCH_PREFIX,
  checkVersion,
  configuredMajor,
  currentVersion,
  nextVersion,
  readChangesets,
} from "./changesets.mjs";

export const REPOSITORY = "jdylanmc/pr-sniper";
export const TAP = "jdylanmc/homebrew-pr-sniper";
export const CI_WORKFLOWS = ["macos.yml", "windows.yml"];
export const RELEASE_WORKFLOW = "release.yml";
export const PIN_PREFIX = "changesets/ci-v";
export const BOT = {
  name: "github-actions[bot]",
  email: "41898282+github-actions[bot]@users.noreply.github.com",
};
// Aggregator steps fail only because a needed job failed.
export const GATE_STEPS = new Set([
  "Require every needed job to succeed",
  "Require successful installer and upgrade fixture builds",
]);
export const LIMITS = {
  attempts: 3,
  pollSeconds: 60,
  stageMinutes: 330,
  settleMinutes: 15,
  tagDispatchGraceMinutes: 5,
};
const VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const FAILED = new Set([
  "failure",
  "cancelled",
  "timed_out",
  "startup_failure",
]);

export class StageError extends Error {
  constructor(stage, message, recovery) {
    super(message);
    this.stage = stage;
    this.recovery = recovery;
  }
}

export const branchFor = (version) => `${RELEASE_BRANCH_PREFIX}${version}`;
export const pinFor = (version) => `${PIN_PREFIX}${version}`;

export function newer(a, b) {
  const [x, y] = [a, b].map((v) => v.split(".").map(Number));
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i];
  return false;
}

const latestOf = (versions) =>
  versions.reduce((max, v) => (newer(v, max) ? v : max), "0.0.0");

export function github({ token, fetch = globalThis.fetch, sleep }) {
  if (!token) throw new Error("GH_TOKEN is required.");
  async function call(
    method,
    path,
    body,
    { missing = false, raw = false } = {},
  ) {
    for (let attempt = 1; ; attempt++) {
      let response;
      try {
        response = await fetch(`https://api.github.com${path}`, {
          method,
          headers: {
            Authorization: `Bearer ${token}`,
            Accept: raw
              ? "application/vnd.github.raw+json"
              : "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "pr-sniper-autorelease",
            ...(body ? { "Content-Type": "application/json" } : {}),
          },
          body: body ? JSON.stringify(body) : undefined,
        });
      } catch (error) {
        if (method === "GET" && attempt < 4) {
          await sleep(attempt * 5000);
          continue;
        }
        throw error;
      }
      if (response.status === 404 && missing) return null;
      if (response.ok) {
        if (response.status === 204) return null;
        return raw ? response.text() : response.json();
      }
      if (method === "GET" && response.status >= 500 && attempt < 4) {
        await sleep(attempt * 5000);
        continue;
      }
      const error = new Error(
        `GitHub ${method} ${path.split("?")[0]} failed with HTTP ${response.status}.`,
      );
      error.status = response.status;
      throw error;
    }
  }
  return {
    get: (path, options) => call("GET", path, undefined, options),
    post: (path, body, options) => call("POST", path, body ?? {}, options),
    put: (path, body, options) => call("PUT", path, body ?? {}, options),
    async graphql(query, variables) {
      const result = await call("POST", "/graphql", { query, variables });
      if (result.errors?.length)
        throw new Error(`GitHub GraphQL failed: ${result.errors[0].type}.`);
      return result.data;
    },
  };
}

const repo = (ctx) => `/repos/${ctx.repository || REPOSITORY}`;

async function fileAt(
  ctx,
  path,
  ref,
  repository = ctx.repository || REPOSITORY,
) {
  return ctx.api.get(
    `/repos/${repository}/contents/${path}?ref=${encodeURIComponent(ref)}`,
    { raw: true, missing: true },
  );
}

async function versionAt(ctx, ref) {
  const text = await fileAt(ctx, "package.json", ref);
  return text === null ? null : JSON.parse(text).version;
}

export async function tagVersions(ctx) {
  const refs = await ctx.api.get(`${repo(ctx)}/git/matching-refs/tags/v`);
  return refs
    .map(({ ref }) => ref.slice("refs/tags/v".length))
    .filter((v) => VERSION.test(v));
}

export async function tagSha(ctx, tag) {
  const ref = await ctx.api.get(`${repo(ctx)}/git/ref/tags/${tag}`, {
    missing: true,
  });
  if (!ref) return null;
  let object = ref.object;
  for (let i = 0; i < 5 && object.type === "tag"; i++)
    object = (await ctx.api.get(`${repo(ctx)}/git/tags/${object.sha}`)).object;
  if (object.type !== "commit")
    throw new StageError(
      "tag",
      `${tag} does not resolve to a commit.`,
      "Inspect the tag; never move it.",
    );
  return object.sha;
}

export async function caskVersion(ctx) {
  const text = await fileAt(ctx, "Casks/pr-sniper.rb", "main", TAP);
  return /^ {2}version "([^"]+)"$/m.exec(text || "")?.[1] || null;
}

async function workflowRuns(ctx, workflow, query) {
  const result = await ctx.api.get(
    `${repo(ctx)}/actions/workflows/${workflow}/runs?${query}&per_page=100`,
  );
  return result.workflow_runs;
}

const latestRun = (runs) =>
  runs.reduce((max, run) => (!max || run.id > max.id ? run : max), null);

async function failedJobs(ctx, run) {
  const { jobs } = await ctx.api.get(
    `${repo(ctx)}/actions/runs/${run.id}/attempts/${run.run_attempt || 1}/jobs?per_page=100`,
  );
  return jobs.filter((job) => FAILED.has(job.conclusion));
}

// Runner loss leaves a failed job with no failed step; aggregators then fail
// only their gate step. Real test failures are never rerun automatically.
export async function runnerLossOnly(ctx, run) {
  const failed = await failedJobs(ctx, run);
  const steps = (job) =>
    (job.steps || []).filter((step) => step.conclusion === "failure");
  return (
    failed.length > 0 &&
    failed.some((job) => steps(job).length === 0) &&
    failed.every((job) => steps(job).every((s) => GATE_STEPS.has(s.name)))
  );
}

const runUrl = (run) => run.html_url;
const env = (ctx) => ctx.env || process.env;
const selfUrl = (ctx) =>
  `${env(ctx).GITHUB_SERVER_URL || "https://github.com"}/${ctx.repository || REPOSITORY}/actions/runs/${env(ctx).GITHUB_RUN_ID || ""}`;

// One pass over the required CI workflows for `sha`. Returns "success" or
// "pending"; dispatches missing runs on `ref` and reruns runner loss.
export async function ciPass(
  ctx,
  sha,
  { stage, accept, ref, beforeDispatch, state },
) {
  let result = "success";
  for (const workflow of CI_WORKFLOWS) {
    const run = latestRun(
      (await workflowRuns(ctx, workflow, `head_sha=${sha}`)).filter(
        (run) => run.head_sha === sha && accept(run),
      ),
    );
    if (!run) {
      result = "pending";
      if (!ref || state.dispatched.has(`${sha} ${workflow}`)) continue;
      await beforeDispatch?.();
      await ctx.api.post(
        `${repo(ctx)}/actions/workflows/${workflow}/dispatches`,
        { ref },
      );
      state.dispatched.add(`${sha} ${workflow}`);
      ctx.log(`Dispatched ${workflow} on ${ref} for ${sha}.`);
      continue;
    }
    if (run.status !== "completed") {
      result = "pending";
      continue;
    }
    if (run.conclusion === "success") {
      state.links?.set(workflow, runUrl(run));
      continue;
    }
    if (
      (run.run_attempt || 1) < LIMITS.attempts &&
      (await runnerLossOnly(ctx, run))
    ) {
      await ctx.api.post(
        `${repo(ctx)}/actions/runs/${run.id}/rerun-failed-jobs`,
      );
      ctx.log(`Rerunning runner-lost jobs of ${runUrl(run)}.`);
      result = "pending";
      continue;
    }
    throw new StageError(
      stage,
      `${workflow} failed on ${sha}: ${runUrl(run)}.`,
      stage === "land"
        ? "Fix main through a normal PR, then re-dispatch Changesets with the same version; the release PR is regenerated on the new main."
        : `Rerun the failed jobs of ${runUrl(run)} if the failure is transient, then re-dispatch Changesets with the same version. Otherwise fix main and release a new version.`,
    );
  }
  return result;
}

async function poll(ctx, stage, step) {
  const deadline = ctx.now() + LIMITS.stageMinutes * 60_000;
  while (ctx.now() < deadline) {
    const value = await step();
    if (value !== undefined) return value;
    await ctx.sleep(LIMITS.pollSeconds * 1000);
  }
  throw new StageError(
    stage,
    `Timed out after ${LIMITS.stageMinutes} minutes.`,
    "Re-dispatch Changesets with the same version; finished stages are skipped.",
  );
}

async function openReleasePrs(ctx) {
  const pulls = await ctx.api.get(
    `${repo(ctx)}/pulls?state=open&base=main&per_page=100`,
  );
  return pulls.filter(
    (pr) =>
      pr.head.ref.startsWith(RELEASE_BRANCH_PREFIX) &&
      pr.head.repo?.full_name === (ctx.repository || REPOSITORY),
  );
}

async function releaseComplete(ctx, version) {
  const run = latestRun(
    await workflowRuns(ctx, RELEASE_WORKFLOW, `branch=v${version}`),
  );
  return run?.conclusion === "success" && (await caskVersion(ctx)) === version;
}

// Chooses the version: requested, an open release PR, an unfinished release
// of main's version, or the next version derived from pending changesets.
export async function plan(ctx, { requested, mainVersion, changesets, major }) {
  const tags = await tagVersions(ctx);
  const latest = latestOf([...tags, mainVersion]);
  const prs = await openReleasePrs(ctx);
  const fresh = (version) => {
    checkVersion(version, latest, major);
    if (changesets.length === 0)
      throw new StageError(
        "plan",
        "No pending .changes/*.md; nothing to release.",
        "Merge PRs with changesets first.",
      );
    return { version, resumed: false };
  };
  if (requested) {
    if (!VERSION.test(requested))
      throw new StageError(
        "plan",
        `Invalid version "${requested}".`,
        "Dispatch with X.Y.Z or omit version.",
      );
    const known =
      tags.includes(requested) ||
      requested === mainVersion ||
      prs.some((pr) => pr.head.ref === branchFor(requested));
    if (known) return { version: requested, resumed: true };
    if (prs.length)
      throw new StageError(
        "plan",
        `Release PR #${prs[0].number} is open for another version.`,
        "Re-dispatch without a version to finish it, or close it first.",
      );
    if (Number(requested.split(".")[0]) !== major)
      throw new StageError(
        "plan",
        `Major version is human-only and fixed at ${major} in .changes/config.json.`,
        "Omit version or keep the configured major.",
      );
    return fresh(requested);
  }
  if (prs.length > 1)
    throw new StageError(
      "plan",
      `Several open release PRs: ${prs.map((pr) => `#${pr.number}`).join(", ")}.`,
      "Close all but one, then re-dispatch.",
    );
  if (prs.length === 1)
    return {
      version: prs[0].head.ref.slice(RELEASE_BRANCH_PREFIX.length),
      resumed: true,
    };
  if (
    !(await releaseComplete(ctx, mainVersion)) &&
    !tags.some((tag) => newer(tag, mainVersion))
  )
    return { version: mainVersion, resumed: true };
  return fresh(nextVersion(latest, changesets, major));
}

export async function releaseCommit(ctx, version) {
  const tagged = await tagSha(ctx, `v${version}`);
  if (tagged) return tagged;
  const commits = await ctx.api.get(
    `${repo(ctx)}/commits?sha=main&path=package.json&per_page=50`,
  );
  for (const commit of commits) {
    if ((await versionAt(ctx, commit.sha)) !== version) continue;
    const parent = commit.parents[0]?.sha;
    if (!parent || (await versionAt(ctx, parent)) !== version)
      return commit.sha;
  }
  throw new StageError(
    "land",
    `No main commit sets version ${version}.`,
    "Dispatch without a version, or with a newer one.",
  );
}

async function mergeOrAutoMerge(ctx, pr) {
  if (pr.mergeable_state === "clean") {
    try {
      await ctx.api.put(`${repo(ctx)}/pulls/${pr.number}/merge`, {
        merge_method: "merge",
        sha: pr.head.sha,
      });
      ctx.log(`Merged #${pr.number}.`);
    } catch (error) {
      ctx.log(`Merge of #${pr.number} not accepted yet (${error.message})`);
    }
    return;
  }
  if (pr.auto_merge) return;
  try {
    await ctx.api.graphql(
      "mutation($id: ID!) { enablePullRequestAutoMerge(input: {pullRequestId: $id, mergeMethod: MERGE}) { clientMutationId } }",
      { id: pr.node_id },
    );
    ctx.log(`Enabled auto-merge on #${pr.number}.`);
  } catch (error) {
    ctx.log(`Auto-merge not enabled on #${pr.number} yet (${error.message})`);
  }
}

// Opens or resumes the release PR, keeps it current with main, runs its
// required CI and lands it through auto-merge. Returns the merge commit.
export async function land(ctx, version, { summary = "" } = {}) {
  const branch = branchFor(version);
  const mainVersion = await versionAt(ctx, "main");
  const pulls = await ctx.api.get(
    `${repo(ctx)}/pulls?state=all&head=${(ctx.repository || REPOSITORY).split("/")[0]}:${branch}&per_page=10`,
  );
  const merged = pulls.find((pr) => pr.merged_at);
  if (merged) {
    ctx.summary("land", `merged #${merged.number}`, merged.html_url);
    return merged.merge_commit_sha;
  }
  if (mainVersion === version || newer(mainVersion, version)) {
    const sha = await releaseCommit(ctx, version);
    ctx.summary("land", `already on main at ${sha.slice(0, 7)}`);
    return sha;
  }
  let pr = pulls.find((pr) => pr.state === "open");
  if (!pr) {
    const closed = pulls.find((pr) => pr.state === "closed");
    const exists = await ctx.api.get(`${repo(ctx)}/git/ref/heads/${branch}`, {
      missing: true,
    });
    if (closed && exists)
      throw new StageError(
        "land",
        `Release PR #${closed.number} was closed without merging.`,
        `Reopen #${closed.number}, or delete branch ${branch}, then re-dispatch.`,
      );
    await ctx.regenerate(version, summary);
    pr = await ctx.api.post(`${repo(ctx)}/pulls`, {
      head: branch,
      base: "main",
      title: `chore(release): v${version}`,
      body: `Release v${version}, opened by [Changesets](${selfUrl(ctx)}). The run dispatches required CI, keeps this branch current and lands it through auto-merge, then tags and releases.`,
    });
  }
  ctx.summary("land", `release PR #${pr.number}`, pr.html_url);
  const state = { dispatched: new Set() };
  let green = null;
  return poll(ctx, "land", async () => {
    pr = await ctx.api.get(`${repo(ctx)}/pulls/${pr.number}`);
    if (pr.merged_at) {
      ctx.summary(
        "land",
        `merged #${pr.number} as ${pr.merge_commit_sha.slice(0, 7)}`,
        pr.html_url,
      );
      return pr.merge_commit_sha;
    }
    if (pr.state === "closed")
      throw new StageError(
        "land",
        `Release PR #${pr.number} was closed without merging.`,
        `Reopen #${pr.number} and re-dispatch.`,
      );
    const behind = await ctx.api.get(
      `${repo(ctx)}/compare/${pr.head.sha}...main`,
    );
    if (behind.ahead_by > 0) {
      ctx.log(`#${pr.number} is behind main; regenerating ${branch}.`);
      await ctx.regenerate(version, summary);
      green = null;
      return undefined;
    }
    const ci = await ciPass(ctx, pr.head.sha, {
      stage: "land",
      accept: () => true,
      ref: branch,
      state,
    });
    await mergeOrAutoMerge(ctx, pr);
    if (ci !== "success") {
      green = null;
      return undefined;
    }
    green ??= ctx.now();
    if (ctx.now() - green > LIMITS.settleMinutes * 60_000)
      throw new StageError(
        "land",
        `Required CI passed on ${pr.head.sha} but #${pr.number} did not merge.`,
        "Check that the dispatched runs' required checks appear on the PR head and that auto-merge is allowed. If they do not attach, configure a GitHub App token (docs/releases.md). Then re-dispatch.",
      );
    return undefined;
  });
}

const mainCi = (version) => (run) =>
  (run.event === "push" && run.head_branch === "main") ||
  (run.event === "workflow_dispatch" &&
    ["main", pinFor(version)].includes(run.head_branch));

// Main CI on the exact release commit. GITHUB_TOKEN merges start no push
// runs, so CI is dispatched on a pin branch that holds exactly that commit.
export async function verify(ctx, version, sha) {
  const pin = pinFor(version);
  const state = { dispatched: new Set(), links: new Map() };
  const beforeDispatch = async () => {
    const ref = await ctx.api.get(`${repo(ctx)}/git/ref/heads/${pin}`, {
      missing: true,
    });
    if (!ref)
      await ctx.api.post(`${repo(ctx)}/git/refs`, {
        ref: `refs/heads/${pin}`,
        sha,
      });
    else if (ref.object.sha !== sha)
      throw new StageError(
        "verify",
        `${pin} points at ${ref.object.sha}, not ${sha}.`,
        `Delete branch ${pin}, then re-dispatch.`,
      );
  };
  await poll(ctx, "verify", async () =>
    (await ciPass(ctx, sha, {
      stage: "verify",
      accept: mainCi(version),
      ref: pin,
      beforeDispatch,
      state,
    })) === "success"
      ? true
      : undefined,
  );
  for (const [workflow, url] of state.links)
    ctx.summary("verify", `${workflow} green on ${sha.slice(0, 7)}`, url);
}

async function ciGreen(ctx, version, sha) {
  for (const workflow of CI_WORKFLOWS) {
    const run = latestRun(
      (await workflowRuns(ctx, workflow, `head_sha=${sha}`)).filter(
        (run) => run.head_sha === sha && mainCi(version)(run),
      ),
    );
    if (run?.status !== "completed" || run.conclusion !== "success")
      return false;
  }
  return true;
}

async function releaseRuns(ctx, tag) {
  return workflowRuns(ctx, RELEASE_WORKFLOW, `branch=${tag}`);
}

async function dispatchRelease(ctx, tag) {
  await ctx.api.post(
    `${repo(ctx)}/actions/workflows/${RELEASE_WORKFLOW}/dispatches`,
    { ref: tag },
  );
  for (let i = 0; i < 30; i++) {
    const run = latestRun(await releaseRuns(ctx, tag));
    if (run) return run;
    await ctx.sleep(5000);
  }
  throw new StageError(
    "tag",
    `${RELEASE_WORKFLOW} run for ${tag} not observed.`,
    "Re-dispatch Changesets with the same version.",
  );
}

// Tags `sha` once its version changed and main CI is green, then starts the
// signed release. Shared by Release tag (workflow_run) and Changesets.
export async function tagCommit(ctx, sha, { required = false, expected } = {}) {
  const commit = await ctx.api.get(`${repo(ctx)}/commits/${sha}`);
  const version = await versionAt(ctx, sha);
  const previous = await versionAt(ctx, commit.parents[0].sha);
  if (version === previous || (expected && version !== expected)) {
    if (required)
      throw new StageError(
        "tag",
        `${sha} does not set version ${expected || version}.`,
        "Re-dispatch Changesets without a version.",
      );
    ctx.log("Not a release commit.");
    return null;
  }
  const tag = `v${version}`;
  let existing = await tagSha(ctx, tag);
  let created = false;
  if (existing && existing !== sha)
    throw new StageError(
      "tag",
      `${tag} points at ${existing}, not ${sha}.`,
      "Never move a tag; release a new version.",
    );
  if (!existing) {
    const changelog = await fileAt(ctx, "CHANGELOG.md", sha);
    if (!changelog?.includes(`## [${version}] - `))
      throw new StageError(
        "tag",
        `CHANGELOG.md at ${sha} has no ${version} section.`,
        "Release through Changesets.",
      );
    if (!(await ciGreen(ctx, version, sha))) {
      if (required)
        throw new StageError(
          "tag",
          `Main CI is not green on ${sha}.`,
          "Re-dispatch Changesets with the same version.",
        );
      ctx.log(`Waiting for main CI on ${sha}.`);
      return null;
    }
    const object = await ctx.api.post(`${repo(ctx)}/git/tags`, {
      tag,
      message: `PR Sniper ${version}`,
      object: sha,
      type: "commit",
      tagger: { ...BOT, date: new Date(ctx.now()).toISOString() },
    });
    try {
      await ctx.api.post(`${repo(ctx)}/git/refs`, {
        ref: `refs/tags/${tag}`,
        sha: object.sha,
      });
      created = true;
    } catch (error) {
      if (error.status !== 422) throw error;
      existing = await tagSha(ctx, tag);
      if (existing !== sha)
        throw new StageError(
          "tag",
          `${tag} was created concurrently at ${existing}.`,
          "Never move a tag; release a new version.",
        );
    }
  }
  let run = latestRun(await releaseRuns(ctx, tag));
  if (
    !run &&
    (created ||
      (await tagAgeMinutes(ctx, tag)) >= LIMITS.tagDispatchGraceMinutes)
  )
    run = await dispatchRelease(ctx, tag);
  ctx.summary(
    "tag",
    `${tag} at ${sha.slice(0, 7)}`,
    `https://github.com/${ctx.repository || REPOSITORY}/releases/tag/${tag}`,
  );
  if (run) ctx.summary("tag", "signed release started", runUrl(run));
  return tag;
}

// The creator dispatches the release at once; others recover a lost dispatch
// only after a grace period, so racing taggers never start two signings.
async function tagAgeMinutes(ctx, tag) {
  const ref = await ctx.api.get(`${repo(ctx)}/git/ref/tags/${tag}`);
  if (ref.object.type !== "tag") return Infinity;
  const object = await ctx.api.get(`${repo(ctx)}/git/tags/${ref.object.sha}`);
  return (ctx.now() - Date.parse(object.tagger.date)) / 60_000;
}

// Waits for the signed release and distribution. Runner loss is rerun; on a
// resume, failed downstream jobs rerun only when signing already succeeded.
export async function releaseStage(ctx, version) {
  const tag = `v${version}`;
  let touched = false;
  return poll(ctx, "release", async () => {
    let run = latestRun(await releaseRuns(ctx, tag));
    if (!run) {
      if ((await tagAgeMinutes(ctx, tag)) < LIMITS.tagDispatchGraceMinutes)
        return undefined;
      run = await dispatchRelease(ctx, tag);
      touched = true;
    }
    if (run.status !== "completed") {
      touched = true;
      return undefined;
    }
    if (run.conclusion === "success") {
      const cask = await caskVersion(ctx);
      if (!cask || newer(version, cask))
        throw new StageError(
          "distribute",
          `Signed release passed but the tap cask is ${cask}.`,
          `Run publish.yml in ${TAP} on main with tag=${tag}.`,
        );
      ctx.summary("release", `signed release ${tag} green`, runUrl(run));
      ctx.summary(
        "distribute",
        `cask ${version}`,
        `https://github.com/${TAP}/blob/main/Casks/pr-sniper.rb`,
      );
      return true;
    }
    const failed = await failedJobs(ctx, run);
    const signed = !failed.some(
      (job) =>
        job.name === "sign" || job.name === "gate" || job.name === "tap-ready",
    );
    const attempt = run.run_attempt || 1;
    if (
      attempt < LIMITS.attempts &&
      signed &&
      (!touched || (await runnerLossOnly(ctx, run)))
    ) {
      await ctx.api.post(
        `${repo(ctx)}/actions/runs/${run.id}/rerun-failed-jobs`,
      );
      ctx.log(`Rerunning failed downstream jobs of ${runUrl(run)}.`);
      touched = true;
      return undefined;
    }
    const names = failed.map((job) => job.name).join(", ") || run.conclusion;
    throw new StageError(
      "release",
      `Signed release failed at ${names}: ${runUrl(run)}.`,
      signed
        ? `Never re-sign ${tag}. While its artifacts are retained (14 days), re-dispatch Changesets with version=${version} to rerun only failed downstream jobs. A published release with a stale cask recovers with publish.yml in ${TAP} (tag=${tag}).`
        : `Nothing was signed or published. Fix the cause (credentials, tap readiness), then rerun ${runUrl(run)}.`,
    );
  });
}

function summaryWriter(environment) {
  return (stage, result, link) => {
    const line = `- **${stage}**: ${link ? `[${result}](${link})` : result}\n`;
    process.stdout.write(line);
    if (environment.GITHUB_STEP_SUMMARY)
      appendFileSync(environment.GITHUB_STEP_SUMMARY, line);
  };
}

function output(environment, values) {
  if (!environment.GITHUB_OUTPUT) return;
  appendFileSync(
    environment.GITHUB_OUTPUT,
    Object.entries(values)
      .map(([key, value]) => `${key}=${value}\n`)
      .join(""),
  );
}

// Builds the release commit on current main and force-pushes the bot branch.
function regenerate(version, summary) {
  const run = (command, args) =>
    execFileSync(command, args, { stdio: "inherit" });
  const branch = branchFor(version);
  run("git", ["fetch", "-q", "origin", "main"]);
  run("git", ["switch", "-q", "-C", branch, "origin/main"]);
  run("npm", ["ci", "--no-audit", "--no-fund"]);
  run("node", [
    "scripts/changesets.mjs",
    "release",
    version,
    "--summary",
    summary,
  ]);
  run("npm", [
    "exec",
    "prettier",
    "--",
    "--check",
    "CHANGELOG.md",
    "package.json",
    "site",
    "src-tauri/tauri.conf.json",
  ]);
  run("git", ["add", "-A"]);
  run("git", [
    "-c",
    `user.name=${BOT.name}`,
    "-c",
    `user.email=${BOT.email}`,
    "commit",
    "-q",
    "-m",
    `chore(release): v${version}`,
  ]);
  run("git", ["push", "-q", "--force", "origin", `HEAD:refs/heads/${branch}`]);
}

export async function main(
  argv = process.argv.slice(2),
  environment = process.env,
) {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const ctx = {
    api: github({ token: environment.GH_TOKEN, sleep }),
    repository: environment.GITHUB_REPOSITORY || REPOSITORY,
    env: environment,
    sleep,
    now: () => Date.now(),
    log: (message) => console.log(message),
    summary: summaryWriter(environment),
    regenerate,
  };
  const [command] = argv;
  const version = environment.VERSION || "";
  const sha = environment.SHA || "";
  try {
    if (command === "plan") {
      const result = await plan(ctx, {
        requested: version,
        mainVersion: currentVersion(),
        changesets: readChangesets(),
        major: configuredMajor(),
      });
      ctx.summary(
        "plan",
        `v${result.version}${result.resumed ? " (resumed)" : ""}`,
      );
      output(environment, { version: result.version });
    } else if (command === "land") {
      output(environment, {
        sha: await land(ctx, version, { summary: environment.SUMMARY || "" }),
      });
    } else if (command === "verify") {
      await verify(ctx, version, sha);
    } else if (command === "tag") {
      await tagCommit(ctx, sha, { required: true, expected: version });
    } else if (command === "tag-commit") {
      await tagCommit(ctx, sha);
    } else if (command === "release") {
      await releaseStage(ctx, version);
    } else {
      console.error(
        "usage: autorelease.mjs plan|land|verify|tag|tag-commit|release",
      );
      return 2;
    }
    return 0;
  } catch (error) {
    const stage = error.stage || command;
    const recovery =
      error.recovery ||
      "Re-dispatch Changesets with the same version; finished stages are skipped.";
    console.log(
      `::error title=Release stage ${stage} failed::${error.message} Recovery: ${recovery}`,
    );
    ctx.summary(stage, `failed: ${error.message} Recovery: ${recovery}`);
    return 1;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url))
  process.exitCode = await main();
