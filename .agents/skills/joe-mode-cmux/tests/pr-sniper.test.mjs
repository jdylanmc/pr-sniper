import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const skills = join(root, ".agents/skills");
const read = (path) => readFileSync(join(root, path), "utf8");
const instructions = read("AGENTS.md");
const team = read(".agents/skills/joe-mode/TEAM.md");
const merge = read(".agents/skills/joe-mode/MERGE.md");
const lock = JSON.parse(read("skills-lock.json"));
const packages = [
  "joe-mode",
  "joe-mode-cmux",
  "doctrine",
  "setup",
  "ship",
  "shepherd",
  "squadron",
];

function markdownFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return markdownFiles(path);
    return entry.isFile() && entry.name.endsWith(".md") ? [path] : [];
  });
}

test("retired adapters are absent and cannot be restored from the lock", () => {
  for (const name of ["joe-mode-paseo", "joe-mode-orca"]) {
    assert.equal(existsSync(join(skills, name)), false, name);
    assert.equal(Object.hasOwn(lock.skills, name), false, name);
  }
  for (const name of packages) {
    assert.equal(
      Object.hasOwn(lock.skills, name),
      false,
      `${name} is repository-owned`,
    );
    assert.ok(existsSync(join(skills, name, "SKILL.md")), name);
  }
});

test("adapted contracts have no broken local file links or retired adapter routes", () => {
  const paths = [
    join(root, "AGENTS.md"),
    join(root, "README.md"),
    ...packages.flatMap((name) => markdownFiles(join(skills, name))),
  ];
  for (const path of paths) {
    const text = readFileSync(path, "utf8");
    assert.doesNotMatch(text, /joe-mode-(?:paseo|orca)\//, path);
    for (const match of text.matchAll(/\[[^\]]*\]\(([^)\s]+)\)/g)) {
      const target = match[1].split("#")[0];
      if (!target || /^[a-z][a-z\d+.-]*:/i.test(target)) continue;
      assert.ok(
        existsSync(resolve(dirname(path), target)),
        `${path} -> ${target}`,
      );
    }
  }
});

test("repository capacity, board and merge authority are preserved", () => {
  assert.match(instructions, /maximum of six developers/);
  assert.match(instructions, /Feature lanes reserve two slots/);
  assert.match(instructions, /pr-sniper-team\/board\.json/);
  assert.match(instructions, /do not create a separate board per worktree/);
  assert.match(team, /Features reserve \*\*two\*\* slots/);
  assert.match(team, /Every writing descendant counts/);
  assert.match(
    team,
    /Permission denial, credentials and human decisions are not fresh-context/,
  );
  assert.match(
    instructions,
    /2026-10-08 human authorization carries that same gate to the CMUX/,
  );
  assert.match(instructions, /This policy-changing PR remains\s+human-merged/);
  for (const text of [merge, instructions]) {
    assert.match(text, /independent/i);
    assert.match(text, /approval votes/);
  }
  assert.match(merge, /independent Roast, successful CI and linting/);
  assert.match(merge, /Rubber duck, then verify/);
  assert.match(merge, /expected\s+head guard/);
  assert.match(merge, /never use admin\/bypass/);
  assert.match(merge, /A queued merge is pending, not merged/);
  assert.match(instructions, /does not activate Joe-mode/);
});

test("shared routes retain native runtime and session-only observation boundaries", () => {
  const invocation = read(".agents/skills/setup/INVOCATION.md");
  const observation = read(".agents/skills/shepherd/OBSERVATION.md");
  const lifecycle = read(".agents/skills/squadron/LIFECYCLE.md");
  assert.match(
    invocation,
    /Registration alone does not make a coordinator a messaging recipient/,
  );
  assert.match(invocation, /generic harness examples never authorize fallback/);
  assert.match(observation, /adapter contract takes precedence/);
  assert.match(observation, /do not create a cron/);
  assert.match(
    lifecycle,
    /close an owned interactive\s+session normally before archive/,
  );
  assert.match(lifecycle, /do not kill it or delete its terminal/);
});

test("both platform CI workflows run the portable package contract suite", () => {
  const packageJson = JSON.parse(read("package.json"));
  for (const file of ["skill-file.test.mjs", "pr-sniper.test.mjs"]) {
    assert.ok(packageJson.scripts["test:skills"].includes(file), file);
  }
  for (const platform of ["macos", "windows"]) {
    assert.match(
      read(`.github/workflows/${platform}.yml`),
      /run: npm run test:skills/,
    );
  }
});
