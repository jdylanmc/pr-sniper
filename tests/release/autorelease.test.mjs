import assert from "node:assert/strict";
import test from "node:test";
import {
  LIMITS,
  StageError,
  land,
  plan,
  releaseStage,
  tagCommit,
  verify,
} from "../../scripts/autorelease.mjs";

const R = "/repos/jdylanmc/pr-sniper";
const TAP = "/repos/jdylanmc/homebrew-pr-sniper";
const MERGE = "m".repeat(40);
const PARENT = "p".repeat(40);
const HEAD = "h".repeat(40);
const NOW = Date.parse("2026-11-01T00:00:00Z");

// In-memory GitHub: versions per ref, tags, PRs, runs per workflow and jobs.
function github(state = {}) {
  const s = {
    versions: { main: "0.5.4", [MERGE]: "0.6.0", [PARENT]: "0.5.4" },
    changelog: "## [0.6.0] - 2026-11-01\n",
    tags: {},
    tagDates: {},
    refs: {},
    pulls: [],
    runs: { "macos.yml": [], "windows.yml": [], "release.yml": [] },
    jobs: {},
    cask: "0.5.4",
    commits: [],
    behind: 0,
    ...state,
  };
  const calls = [];
  let id = 100;
  const route = (method, path, body) => {
    const url = new URL(`https://x${path}`);
    const p = url.pathname;
    const q = url.searchParams;
    if (method === "GET") {
      if (p === `${R}/contents/package.json`) {
        const v = s.versions[q.get("ref")];
        return v === undefined ? null : JSON.stringify({ version: v });
      }
      if (p === `${R}/contents/CHANGELOG.md`) return s.changelog;
      if (p === `${TAP}/contents/Casks/pr-sniper.rb`)
        return `cask "pr-sniper" do\n  version "${s.cask}"\nend\n`;
      if (p === `${R}/git/matching-refs/tags/v`)
        return Object.keys(s.tags).map((t) => ({ ref: `refs/tags/${t}` }));
      let m = /\/git\/ref\/tags\/(.+)$/.exec(p);
      if (m)
        return s.tags[m[1]]
          ? { object: { type: "tag", sha: `obj-${m[1]}` } }
          : null;
      m = /\/git\/tags\/obj-(.+)$/.exec(p);
      if (m)
        return {
          object: { type: "commit", sha: s.tags[m[1]] },
          tagger: { date: s.tagDates[m[1]] || new Date(0).toISOString() },
        };
      m = /\/git\/ref\/heads\/(.+)$/.exec(p);
      if (m) return s.refs[m[1]] ? { object: { sha: s.refs[m[1]] } } : null;
      if (p === `${R}/pulls`) {
        if (q.get("state") === "open")
          return s.pulls.filter((pr) => pr.state === "open");
        return s.pulls.filter(
          (pr) => `jdylanmc:${pr.head.ref}` === q.get("head"),
        );
      }
      m = /\/pulls\/(\d+)$/.exec(p);
      if (m) return s.pulls.find((pr) => pr.number === Number(m[1]));
      if (/\/compare\//.test(p)) return { ahead_by: s.behind };
      m = /\/actions\/workflows\/(.+)\/runs$/.exec(p);
      if (m) {
        const runs = s.runs[m[1]].filter(
          (run) =>
            (!q.get("head_sha") || run.head_sha === q.get("head_sha")) &&
            (!q.get("branch") || run.head_branch === q.get("branch")),
        );
        return { workflow_runs: runs };
      }
      m = /\/actions\/runs\/(\d+)\/attempts\/\d+\/jobs$/.exec(p);
      if (m) return { jobs: s.jobs[m[1]] || [] };
      m = /\/commits\/([^/]+)$/.exec(p);
      if (m) return { sha: m[1], parents: [{ sha: PARENT }] };
      if (p === `${R}/commits`) return s.commits;
    }
    if (method === "POST") {
      let m = /\/actions\/workflows\/(.+)\/dispatches$/.exec(p);
      if (m) {
        const ref = body.ref;
        const sha =
          s.tags[ref] ||
          s.refs[ref] ||
          s.pulls.find((pr) => pr.head.ref === ref)?.head.sha;
        s.runs[m[1]].push({
          id: ++id,
          head_sha: sha,
          head_branch: ref,
          event: "workflow_dispatch",
          status: "queued",
          html_url: `run/${id}`,
        });
        return null;
      }
      m = /\/actions\/runs\/(\d+)\/rerun-failed-jobs$/.exec(p);
      if (m) {
        for (const runs of Object.values(s.runs))
          for (const run of runs)
            if (run.id === Number(m[1]))
              Object.assign(run, {
                status: "queued",
                run_attempt: (run.run_attempt || 1) + 1,
              });
        return null;
      }
      if (p === `${R}/git/refs`) {
        const name = body.ref.replace(/^refs\/(heads|tags)\//, "");
        if (body.ref.startsWith("refs/tags/")) {
          if (s.tags[name]) {
            const error = new Error("exists");
            error.status = 422;
            throw error;
          }
          s.tags[name] = s.tagObjects[body.sha];
          s.tagDates[name] = new Date(NOW).toISOString();
        } else s.refs[name] = body.sha;
        return {};
      }
      if (p === `${R}/git/tags`) {
        s.tagObjects = { ...s.tagObjects, [`o-${body.tag}`]: body.object };
        return { sha: `o-${body.tag}` };
      }
      if (p === `${R}/pulls`) {
        const pr = {
          number: 7,
          state: "open",
          node_id: "PR_7",
          html_url: "pr/7",
          head: {
            ref: body.head,
            sha: s.refs[body.head],
            repo: { full_name: "jdylanmc/pr-sniper" },
          },
          auto_merge: null,
        };
        s.pulls.push(pr);
        return pr;
      }
      if (p === "/graphql") {
        s.pulls.forEach((pr) => (pr.auto_merge = { enabled_by: "bot" }));
        return { data: {} };
      }
    }
    if (method === "PUT" && /\/pulls\/\d+\/merge$/.test(p))
      return { merged: true };
    throw new Error(`unrouted ${method} ${path}`);
  };
  const call = (method) => async (path, bodyOrOptions, options) => {
    const body = method === "GET" ? undefined : bodyOrOptions;
    const opts = (method === "GET" ? bodyOrOptions : options) || {};
    calls.push([method, path, body]);
    const result = route(method, path, body);
    if (result === null && !opts.missing && method === "GET")
      throw Object.assign(new Error("404"), { status: 404 });
    return result;
  };
  const api = {
    get: call("GET"),
    post: call("POST"),
    put: call("PUT"),
    graphql: (query, variables) =>
      call("POST")("/graphql", { query, variables }),
  };
  return { s, calls, api };
}

function context(gh, { tick, regenerate } = {}) {
  let now = NOW;
  const rows = [];
  const ctx = {
    api: gh.api,
    repository: "jdylanmc/pr-sniper",
    env: {},
    now: () => now,
    sleep: async (ms) => {
      now += ms;
      await tick?.(gh.s);
    },
    log: () => {},
    summary: (stage, result, link) => rows.push([stage, result, link]),
    regenerate: async (version) => {
      regenerate?.(version);
      gh.s.refs[`changesets/release-v${version}`] = gh.s.nextHead || HEAD;
      for (const pr of gh.s.pulls) pr.head.sha = gh.s.refs[pr.head.ref];
    },
  };
  return { ctx, rows };
}

const mutations = (gh) => gh.calls.filter(([method]) => method !== "GET");
const complete = (run, conclusion = "success") =>
  Object.assign(run, { status: "completed", conclusion });

test("plan derives Y/Z from pending changesets and resumes unfinished releases", async () => {
  const base = { mainVersion: "0.5.4", major: 0 };
  const done = () => {
    const gh = github({ tags: { "v0.5.4": PARENT } });
    gh.s.runs["release.yml"].push({
      id: 1,
      head_branch: "v0.5.4",
      conclusion: "success",
    });
    return gh;
  };
  let { ctx } = context(done());
  assert.deepEqual(
    await plan(ctx, {
      ...base,
      requested: "",
      changesets: [{ type: "fixed" }],
    }),
    { version: "0.5.5", resumed: false },
  );
  assert.equal(
    (
      await plan(ctx, {
        ...base,
        requested: "",
        changesets: [{ type: "fixed" }, { type: "added" }],
      })
    ).version,
    "0.6.0",
  );
  await assert.rejects(
    plan(ctx, { ...base, requested: "", changesets: [] }),
    /nothing to release/,
  );
  await assert.rejects(
    plan(ctx, { ...base, requested: "1.0.0", changesets: [{ type: "added" }] }),
    /human-only/,
  );
  await assert.rejects(
    plan(ctx, { ...base, requested: "0.5.3", changesets: [{ type: "added" }] }),
    /newer/,
  );
  assert.deepEqual(
    await plan(ctx, { ...base, requested: "0.5.4", changesets: [] }),
    {
      version: "0.5.4",
      resumed: true,
    },
  );
  const unfinished = github({ tags: { "v0.5.4": PARENT }, cask: "0.5.3" });
  ({ ctx } = context(unfinished));
  assert.equal(
    (
      await plan(ctx, {
        ...base,
        requested: "",
        changesets: [{ type: "added" }],
      })
    ).version,
    "0.5.4",
  );
  const open = done();
  open.s.pulls.push({
    number: 3,
    state: "open",
    head: {
      ref: "changesets/release-v0.5.6",
      repo: { full_name: "jdylanmc/pr-sniper" },
    },
  });
  ({ ctx } = context(open));
  assert.deepEqual(
    await plan(ctx, {
      ...base,
      requested: "",
      changesets: [{ type: "added" }],
    }),
    {
      version: "0.5.6",
      resumed: true,
    },
  );
  await assert.rejects(
    plan(ctx, { ...base, requested: "0.7.0", changesets: [{ type: "added" }] }),
    /another version/,
  );
  const released = done();
  released.s.versions.main = "0.6.0";
  released.s.tags["v0.6.0"] = MERGE;
  await assert.rejects(
    plan(context(released).ctx, {
      ...base,
      requested: "",
      changesets: [{ type: "added" }],
    }),
    (error) =>
      error.stage === "plan" && /older than current main/.test(error.message),
  );
});

test("land opens the release PR, dispatches required CI on it, auto-merges and resumes merged PRs", async () => {
  const gh = github();
  let regenerated = 0;
  const { ctx, rows } = context(gh, {
    regenerate: () => regenerated++,
    tick: (s) => {
      for (const run of [...s.runs["macos.yml"], ...s.runs["windows.yml"]])
        complete(run);
      const pr = s.pulls[0];
      if (
        pr?.auto_merge &&
        s.runs["macos.yml"].length &&
        s.runs["windows.yml"].length
      )
        Object.assign(pr, {
          merged_at: "now",
          merge_commit_sha: MERGE,
          state: "closed",
        });
    },
  });
  assert.equal(await land(ctx, "0.6.0"), MERGE);
  assert.equal(regenerated, 1);
  const dispatched = mutations(gh).filter(([, path]) =>
    path.endsWith("/dispatches"),
  );
  assert.deepEqual(
    dispatched.map(([, path, body]) => [path.split("/").at(-2), body.ref]),
    [
      ["macos.yml", "changesets/release-v0.6.0"],
      ["windows.yml", "changesets/release-v0.6.0"],
    ],
  );
  assert.ok(mutations(gh).some(([, path]) => path === "/graphql"));
  assert.ok(
    rows.some(
      ([stage, result]) => stage === "land" && /merged #7/.test(result),
    ),
  );
  const before = gh.calls.length;
  assert.equal(await land(ctx, "0.6.0"), MERGE);
  assert.equal(regenerated, 1, "a merged PR is never regenerated");
  assert.equal(mutations({ calls: gh.calls.slice(before) }).length, 0);
});

test("land regenerates a behind branch, reruns runner loss and stops on real CI failure", async () => {
  const gh = github({ behind: 1 });
  let regenerated = 0;
  const { ctx } = context(gh, {
    regenerate: () => {
      regenerated++;
      gh.s.behind = 0;
      gh.s.nextHead = "n".repeat(40);
    },
    tick: (s) => {
      for (const run of s.runs["macos.yml"]) {
        if (run.status !== "queued") continue;
        if ((run.run_attempt || 1) === 1) {
          complete(run, "failure");
          s.jobs[run.id] = [
            { name: "macos-native", conclusion: "failure", steps: [] },
            {
              name: "macos",
              conclusion: "failure",
              steps: [
                {
                  name: "Require every needed job to succeed",
                  conclusion: "failure",
                },
              ],
            },
          ];
        } else {
          complete(run, "failure");
          s.jobs[run.id] = [
            {
              name: "macos-native",
              conclusion: "failure",
              steps: [{ name: "Test", conclusion: "failure" }],
            },
          ];
        }
      }
      for (const run of s.runs["windows.yml"]) complete(run);
    },
  });
  gh.s.refs["changesets/release-v0.6.0"] = HEAD;
  gh.s.pulls.push({
    number: 7,
    state: "open",
    node_id: "PR_7",
    auto_merge: {},
    html_url: "pr/7",
    head: {
      ref: "changesets/release-v0.6.0",
      sha: HEAD,
      repo: { full_name: "jdylanmc/pr-sniper" },
    },
  });
  await assert.rejects(land(ctx, "0.6.0"), (error) => {
    assert.ok(error instanceof StageError);
    assert.equal(error.stage, "land");
    assert.match(error.message, /macos\.yml failed/);
    return true;
  });
  assert.equal(regenerated, 1);
  assert.ok(
    gh.s.runs["macos.yml"].every((run) => run.head_sha === "n".repeat(40)),
  );
  assert.equal(
    mutations(gh).filter(([, path]) => path.endsWith("/rerun-failed-jobs"))
      .length,
    1,
  );
});

test("verify dispatches main CI on a pin branch at the exact merge commit, once", async () => {
  const gh = github();
  const { ctx } = context(gh, {
    tick: (s) =>
      [...s.runs["macos.yml"], ...s.runs["windows.yml"]].forEach((run) =>
        complete(run),
      ),
  });
  await verify(ctx, "0.6.0", MERGE);
  assert.equal(gh.s.refs["changesets/ci-v0.6.0"], MERGE);
  for (const workflow of ["macos.yml", "windows.yml"])
    assert.deepEqual(
      gh.s.runs[workflow].map((run) => [run.head_sha, run.head_branch]),
      [[MERGE, "changesets/ci-v0.6.0"]],
    );
  const before = mutations(gh).length;
  await verify(ctx, "0.6.0", MERGE);
  assert.equal(mutations(gh).length, before, "green CI is not re-dispatched");
  const moved = github({ refs: { "changesets/ci-v0.6.0": HEAD } });
  await assert.rejects(verify(context(moved).ctx, "0.6.0", MERGE), /points at/);
  const pushed = github();
  for (const workflow of ["macos.yml", "windows.yml"])
    pushed.s.runs[workflow].push({
      id: 1,
      head_sha: MERGE,
      head_branch: "main",
      event: "push",
      status: "completed",
      conclusion: "success",
    });
  await verify(context(pushed).ctx, "0.6.0", MERGE);
  assert.equal(mutations(pushed).length, 0);
});

test("tag creates one annotated tag after green main CI and starts one signed release", async () => {
  const green = (gh) => {
    for (const workflow of ["macos.yml", "windows.yml"])
      gh.s.runs[workflow].push({
        id: 1,
        head_sha: MERGE,
        head_branch: "changesets/ci-v0.6.0",
        event: "workflow_dispatch",
        status: "completed",
        conclusion: "success",
      });
  };
  const pending = github();
  await assert.rejects(
    tagCommit(context(pending).ctx, MERGE, {
      required: true,
      expected: "0.6.0",
    }),
    /not green/,
  );
  assert.equal(await tagCommit(context(pending).ctx, MERGE), null);
  assert.equal(mutations(pending).length, 0);
  const gh = github();
  green(gh);
  assert.equal(
    await tagCommit(context(gh).ctx, MERGE, {
      required: true,
      expected: "0.6.0",
    }),
    "v0.6.0",
  );
  assert.equal(gh.s.tags["v0.6.0"], MERGE);
  assert.equal(gh.s.runs["release.yml"].length, 1);
  assert.equal(gh.s.runs["release.yml"][0].head_branch, "v0.6.0");
  const again = mutations(gh).length;
  await tagCommit(context(gh).ctx, MERGE);
  assert.equal(
    mutations(gh).length,
    again,
    "existing tag and release run are reused",
  );
  const young = github({
    tags: { "v0.6.0": MERGE },
    tagDates: { "v0.6.0": new Date(NOW).toISOString() },
  });
  await tagCommit(context(young).ctx, MERGE);
  assert.equal(
    young.s.runs["release.yml"].length,
    0,
    "a fresh tag's creator dispatches, not a racer",
  );
  const old = github({ tags: { "v0.6.0": MERGE } });
  await tagCommit(context(old).ctx, MERGE);
  assert.equal(
    old.s.runs["release.yml"].length,
    1,
    "a lost dispatch is recovered",
  );
  const moved = github({ tags: { "v0.6.0": HEAD } });
  await assert.rejects(tagCommit(context(moved).ctx, MERGE), (error) =>
    /Never move a tag/.test(error.recovery),
  );
  const plain = github({ versions: { [MERGE]: "0.5.4", [PARENT]: "0.5.4" } });
  assert.equal(await tagCommit(context(plain).ctx, MERGE), null);
});

test("release reruns only downstream jobs after signing and confirms the cask", async () => {
  const run = (conclusion, failed) => ({
    id: 9,
    head_branch: "v0.6.0",
    status: "completed",
    conclusion,
    run_attempt: 1,
    html_url: "run/9",
    failed,
  });
  const gh = github({ tags: { "v0.6.0": MERGE }, cask: "0.5.4" });
  gh.s.runs["release.yml"].push(run("failure"));
  gh.s.jobs[9] = [
    {
      name: "tap",
      conclusion: "failure",
      steps: [{ name: "Verify", conclusion: "failure" }],
    },
  ];
  const { ctx, rows } = context(gh, {
    tick: (s) => {
      const r = s.runs["release.yml"][0];
      if (r.status === "queued") {
        complete(r);
        s.cask = "0.6.0";
      }
    },
  });
  assert.equal(await releaseStage(ctx, "0.6.0"), true);
  assert.equal(
    mutations(gh).filter(([, path]) => path.endsWith("/rerun-failed-jobs"))
      .length,
    1,
  );
  assert.ok(rows.some(([stage]) => stage === "distribute"));

  const unsigned = github({ tags: { "v0.6.0": MERGE } });
  unsigned.s.runs["release.yml"].push(run("failure"));
  unsigned.s.jobs[9] = [
    {
      name: "sign",
      conclusion: "failure",
      steps: [{ name: "Sign", conclusion: "failure" }],
    },
  ];
  await assert.rejects(
    releaseStage(context(unsigned).ctx, "0.6.0"),
    (error) => {
      assert.equal(error.stage, "release");
      assert.match(error.recovery, /Nothing was signed/);
      return true;
    },
  );
  assert.equal(
    mutations(unsigned).length,
    0,
    "signing is never rerun automatically",
  );

  const exhausted = github({ tags: { "v0.6.0": MERGE } });
  exhausted.s.runs["release.yml"].push({
    ...run("failure"),
    run_attempt: LIMITS.attempts,
  });
  exhausted.s.jobs[9] = [
    {
      name: "tap",
      conclusion: "failure",
      steps: [{ name: "Verify", conclusion: "failure" }],
    },
  ];
  await assert.rejects(releaseStage(context(exhausted).ctx, "0.6.0"), (error) =>
    /Never re-sign/.test(error.recovery),
  );

  const stale = github({ tags: { "v0.6.0": MERGE }, cask: "0.5.4" });
  stale.s.runs["release.yml"].push(run("success"));
  await assert.rejects(
    releaseStage(context(stale).ctx, "0.6.0"),
    (error) => error.stage === "distribute",
  );
});
