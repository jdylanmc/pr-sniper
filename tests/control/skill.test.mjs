import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";

const root = path.resolve("skills/pr-sniper");

async function files(directory, prefix = "") {
  const found = [];
  for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
    const name = path.posix.join(prefix, entry.name);
    if (entry.isDirectory())
      found.push(...(await files(path.join(directory, entry.name), name)));
    else found.push(name);
  }
  return found.sort();
}

test("skills CLI package is thin orchestration with no bundled server", async () => {
  assert.deepEqual(await files(root), ["SKILL.md", "references/control.md"]);
  const skill = await fs.readFile(path.join(root, "SKILL.md"), "utf8");
  const frontmatter = skill.match(/^---\n([\s\S]*?)\n---\n/)?.[1];
  assert.ok(frontmatter, "SKILL.md needs YAML frontmatter");
  assert.match(frontmatter, /^name: pr-sniper$/m);
  assert.match(frontmatter, /^description: \S.{20,1024}$/m);
  assert.match(skill, /protocol `1`/);
  assert.match(skill, /update the PR Sniper app/);
  for (const [, target] of skill.matchAll(/\]\(([^)#]+)\)/g))
    await fs.access(path.join(root, target));
});

test("the app-registered server accepts only an absolute executable", async () => {
  const { useExecutable } =
    await import("../../src-control/control-client.mjs");
  assert.throws(() => useExecutable("relative/pr-sniper"), {
    code: "invalid_executable",
  });
  assert.throws(() => useExecutable(undefined), {
    code: "invalid_executable",
  });
});
