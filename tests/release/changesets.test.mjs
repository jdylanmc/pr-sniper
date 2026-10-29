import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import * as prettier from "prettier";
import {
  ChangesetError,
  VERSION_FIELDS,
  applyChangesets,
  consume,
  moveUnreleased,
  parseChangeset,
  readChangesets,
  release,
  validate,
} from "../../scripts/changesets.mjs";

const HEAD = `# Changelog

Notable changes are recorded here.

## Unreleased
`;
const HISTORY = `
## 0.1.0 - 2026-01-01

### Added

- First release.
`;

function fixture(files = {}) {
  const root = join(process.cwd(), `.changesets-test-${randomUUID()}`);
  mkdirSync(join(root, ".changes"), { recursive: true });
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  }
  return root;
}

const entry = (type, body, extra = "") =>
  `---\ntype: ${type}\n${extra}---\n\n${body}\n`;

async function assertPrettierStable(text) {
  const formatted = await prettier.format(text, { parser: "markdown" });
  assert.equal(formatted, text);
}

test("parses front matter and rejects malformed changesets", () => {
  assert.deepEqual(
    parseChangeset(entry("Fixed", "Fix it.", "issue: 12, #13\npr: 14\n")),
    { type: "fixed", issues: [12, 13], prs: [14], body: "Fix it." },
  );
  assert.deepEqual(
    parseChangeset(entry("added", "Add it.").replace(/\n/g, "\r\n")).body,
    "Add it.",
  );
  for (const bad of [
    "Add it.\n",
    entry("new", "Add it."),
    entry("added", ""),
    entry("added", "Add it.", "owner: me\n"),
    entry("added", "Add it.", "type: fixed\n"),
    entry("added", "Add it.", "issue: abc\n"),
    entry("added", "### Added\n\n- Add it."),
    entry("fixed", "Fix queue.\n\nPreserve retries."),
  ])
    assert.throws(() => parseChangeset(bad, "bad.md"), ChangesetError, bad);
});

test("consume appends entries under ordered subsections, deletes files and is idempotent", async () => {
  const root = fixture({
    "CHANGELOG.md": `${HEAD}\n### Fixed\n\n- Existing fix.\n${HISTORY}`,
    ".changes/README.md": "# Changes\n",
    ".changes/b-fix.md": entry("fixed", "Fix the queue.", "issue: 7\n"),
    ".changes/a-sec.md": entry("security", "- Redact tokens (#8)."),
    ".changes/c-add.md": entry(
      "added",
      "Add a long entry\nthat wraps.",
      "pr: 9\n",
    ),
  });
  try {
    assert.deepEqual(consume(root), ["a-sec.md", "b-fix.md", "c-add.md"]);
    const text = readFileSync(join(root, "CHANGELOG.md"), "utf8");
    assert.equal(
      text,
      `${HEAD}
### Added

- Add a long entry
  that wraps (#9).

### Fixed

- Existing fix.
- Fix the queue (#7).

### Security

- Redact tokens (#8).
${HISTORY}`,
    );
    await assertPrettierStable(text);
    assert.deepEqual(readdirSync(join(root, ".changes")), ["README.md"]);
    assert.deepEqual(consume(root), []);
    assert.equal(readFileSync(join(root, "CHANGELOG.md"), "utf8"), text);
    writeFileSync(
      join(root, ".changes/b-fix.md"),
      entry("fixed", "Fix the queue.", "issue: 7\n"),
    );
    assert.deepEqual(consume(root), ["b-fix.md"]);
    assert.equal(readFileSync(join(root, "CHANGELOG.md"), "utf8"), text);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("consume skips only complete duplicate entries in the same subsection", () => {
  const existing = [
    "- Fix queue (#7).",
    "- Fix queue. Preserve retry state.",
    "- Keep",
    "  wrapped.",
  ];
  const changelog = `${HEAD}\n### Fixed\n\n${existing.join("\n")}\n${HISTORY}`;
  const fixed = (body) => ({ type: "fixed", issues: [], prs: [], body });
  const after = applyChangesets(changelog, [
    fixed("Fix queue"),
    fixed("Fix queue."),
    fixed("Keep\nwrapped."),
    { ...fixed("Fix queue (#7)."), type: "added" },
  ]);
  assert.equal(
    after,
    `${HEAD}\n### Added\n\n- Fix queue (#7).\n\n### Fixed\n\n${[
      ...existing,
      "- Fix queue",
      "- Fix queue.",
    ].join("\n")}\n${HISTORY}`,
  );
  assert.equal(applyChangesets(after, [fixed("Fix queue")]), after);
});

test("consume preserves the real changelog history byte for byte", async () => {
  const real = readFileSync("CHANGELOG.md", "utf8");
  const after = applyChangesets(real, [
    { type: "changed", issues: [], prs: [], body: "Adopt changesets-test." },
  ]);
  const released = (text) => text.slice(text.search(/^## \d/m));
  assert.equal(released(after), released(real));
  const unreleased = after.slice(0, after.search(/^## \d/m));
  assert.match(unreleased, /^- Adopt changesets-test\.$/m);
  assert.ok(unreleased.indexOf("### Changed") < unreleased.indexOf("- Adopt"));
  await assertPrettierStable(after);
  assert.equal(applyChangesets(real, []), real);
});

test("release moves Unreleased and bumps every checked-in version field", async () => {
  const root = fixture();
  const files = [...new Set(VERSION_FIELDS.map(([file]) => file))];
  try {
    for (const file of files) {
      mkdirSync(dirname(join(root, file)), { recursive: true });
      cpSync(file, join(root, file));
    }
    writeFileSync(
      join(root, ".changes/pr-1.md"),
      entry("added", "Ship changesets.", "issue: 291\n"),
    );
    const previous = JSON.parse(readFileSync("package.json", "utf8")).version;
    writeFileSync(
      join(root, "CHANGELOG.md"),
      `${HEAD}${HISTORY.replace("0.1.0", previous)}`,
    );
    const [major, minor] = previous.split(".").map(Number);
    const version = `${major}.${minor + 1}.0`;
    let built = 0;
    const result = release(root, version, {
      date: "2026-11-01",
      summary: "Changesets by file.",
      build: () => built++,
    });
    assert.deepEqual(result.consumed, ["pr-1.md"]);
    assert.equal(built, 1);
    const changelog = readFileSync(join(root, "CHANGELOG.md"), "utf8");
    assert.match(
      changelog,
      new RegExp(
        `## Unreleased\\n\\n## ${version} - 2026-11-01\\n\\nChangesets by file\\.\\n\\n### Added\\n\\n- Ship changesets \\(#291\\)\\.\\n\\n## ${previous} - `,
      ),
    );
    await assertPrettierStable(changelog);
    for (const file of files) {
      const before = readFileSync(file, "utf8").split("\n");
      const after = readFileSync(join(root, file), "utf8").split("\n");
      assert.equal(after.length, before.length, file);
      const changed = after.filter((line, i) => line !== before[i]);
      assert.equal(
        changed.length,
        VERSION_FIELDS.filter(([name]) => name === file).length,
        file,
      );
      for (const line of changed) assert.ok(line.includes(version), file);
    }
    const python = process.platform === "win32" ? "python" : "python3";
    const checked = execFileSync(
      python,
      [
        "-B",
        "-c",
        "import sys; from pathlib import Path; sys.path.insert(0, 'scripts'); import release; print(release.config_version(Path(sys.argv[1])))",
        root,
      ],
      { encoding: "utf8" },
    ).trim();
    assert.equal(checked, version);
    assert.throws(
      () => release(root, version, { build: () => {} }),
      ChangesetError,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("release refuses an empty Unreleased section or duplicate version", () => {
  assert.throws(
    () => moveUnreleased(`${HEAD}${HISTORY}`, "0.2.0", "2026-11-01"),
    /Unreleased is empty/,
  );
  assert.throws(
    () => moveUnreleased(`${HEAD}\n- x\n${HISTORY}`, "0.1.0", "2026-11-01"),
    /already exists/,
  );
  assert.throws(() => moveUnreleased(HEAD, "v1.0.0", "2026-11-01"));
});

test("validate rejects direct changelog edits unless bot or opted out", () => {
  const root = fixture({ ".changes/ok.md": entry("fixed", "Fix it.") });
  try {
    const edit = [
      { status: "M", path: "CHANGELOG.md" },
      { status: "A", path: ".changes/ok.md" },
    ];
    assert.equal(validate({ root, changed: edit }).errors.length, 1);
    assert.deepEqual(
      validate({ root, changed: edit, labels: ["no-changeset"] }).errors,
      [],
    );
    assert.deepEqual(
      validate({ root, changed: edit, headRef: "changesets/consume" }).errors,
      [],
    );
    assert.equal(
      validate({
        root,
        changed: edit,
        headRef: "changesets/consume",
        sameRepo: false,
      }).errors.length,
      1,
    );
    assert.deepEqual(
      validate({ root, changed: edit, author: "github-actions[bot]" }).errors,
      [],
    );
    const plain = validate({
      root,
      changed: [{ status: "M", path: "src/main.ts" }],
    });
    assert.deepEqual(plain.errors, []);
    assert.equal(plain.warnings.length, 1);
    writeFileSync(join(root, ".changes/Bad Name.md"), entry("fixed", "x"));
    assert.equal(validate({ root }).errors.length, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("checked-in changesets are valid", () => {
  assert.ok(existsSync(".changes/README.md"));
  readChangesets(".");
});
