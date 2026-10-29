#!/usr/bin/env node
// Changesets by file (#291): validate, consume and release `.changes/*.md`.
import { execFileSync } from "node:child_process";
import {
  existsSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const CHANGES_DIR = ".changes";
export const CHANGELOG = "CHANGELOG.md";
export const TYPES = [
  "added",
  "changed",
  "deprecated",
  "removed",
  "fixed",
  "security",
];
export const OPT_OUT_LABEL = "no-changeset";
export const BOT_BRANCH_PREFIX = "changesets/";
export const BOT_AUTHORS = ["github-actions[bot]"];
const RESERVED = new Set(["README.md"]);
const NAME = /^[a-z0-9][a-z0-9._-]*\.md$/;
const VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const REF = /^#?([1-9]\d*)$/;
const UNRELEASED = /^## \[?Unreleased\]?[ \t]*$/m;

export class ChangesetError extends Error {}

const lf = (text) => text.replace(/\r\n?/g, "\n");
const heading = (type) => `### ${type[0].toUpperCase()}${type.slice(1)}`;

function refs(value, key, file) {
  const items = String(value)
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
  if (items.length === 0) throw new ChangesetError(`${file}: empty ${key}`);
  return items.map((item) => {
    const match = REF.exec(item);
    if (!match) throw new ChangesetError(`${file}: invalid ${key} "${item}"`);
    return Number(match[1]);
  });
}

export function parseChangeset(text, file = "changeset") {
  const source = lf(text);
  const match = /^---\n([\s\S]*?)\n---\n([\s\S]*)$/.exec(source);
  if (!match) throw new ChangesetError(`${file}: missing front matter`);
  const fields = {};
  for (const line of match[1].split("\n")) {
    if (!line.trim()) continue;
    const pair = /^([a-z]+):[ \t]*(.*?)[ \t]*$/.exec(line);
    if (!pair) throw new ChangesetError(`${file}: invalid front matter line`);
    const [, key, value] = pair;
    if (!["type", "issue", "pr"].includes(key))
      throw new ChangesetError(`${file}: unknown key "${key}"`);
    if (Object.hasOwn(fields, key))
      throw new ChangesetError(`${file}: duplicate key "${key}"`);
    fields[key] = value.replace(/^(["'])(.*)\1$/, "$2");
  }
  const type = (fields.type || "").toLowerCase();
  if (!TYPES.includes(type))
    throw new ChangesetError(`${file}: type must be one of ${TYPES.join("|")}`);
  const body = match[2].trim();
  if (!body) throw new ChangesetError(`${file}: empty body`);
  if (/^#/m.test(body))
    throw new ChangesetError(`${file}: body must not contain headings`);
  return {
    type,
    issues: fields.issue === undefined ? [] : refs(fields.issue, "issue", file),
    prs: fields.pr === undefined ? [] : refs(fields.pr, "pr", file),
    body,
  };
}

// One bullet per changeset; refs appended once unless the text already cites them.
export function renderEntry({ body, issues, prs }) {
  const lines = body.split("\n").map((line) => line.trimEnd());
  const bullet = lines[0].startsWith("- ");
  const text = bullet
    ? lines
    : lines.map((line, index) =>
        index === 0 ? `- ${line}` : line ? `  ${line}` : line,
      );
  const missing = [...new Set([...issues, ...prs])].filter(
    (n) => !new RegExp(`#${n}(?!\\d)`).test(body),
  );
  if (missing.length) {
    const suffix = missing.map((n) => `#${n}`).join(", ");
    const last = text.length - 1;
    text[last] = text[last].replace(/\.?$/, (dot) => ` (${suffix})${dot}`);
  }
  return text.join("\n");
}

export function listChangesets(root = ".") {
  const directory = join(root, CHANGES_DIR);
  if (!existsSync(directory)) return [];
  return readdirSync(directory)
    .filter((name) => name.endsWith(".md") && !RESERVED.has(name))
    .sort();
}

export function readChangesets(root = ".") {
  return listChangesets(root).map((name) => {
    if (!NAME.test(name))
      throw new ChangesetError(
        `${name}: use lowercase letters, digits, ".", "_" or "-"`,
      );
    const path = join(root, CHANGES_DIR, name);
    return { name, ...parseChangeset(readFileSync(path, "utf8"), name) };
  });
}

function unreleasedBounds(text) {
  const match = UNRELEASED.exec(text);
  if (!match) throw new ChangesetError(`${CHANGELOG}: missing ## Unreleased`);
  const start = match.index + match[0].length;
  const next = /^## /m.exec(text.slice(start));
  return {
    head: match.index,
    start,
    end: next ? start + next.index : text.length,
  };
}

// Parse Unreleased into preamble + typed subsections, preserving unknown text.
function parseSection(content) {
  const blocks = { preamble: [], sections: new Map() };
  let current = blocks.preamble;
  for (const line of content.split("\n")) {
    const match = /^### (\w+)\s*$/.exec(line);
    const type = match?.[1].toLowerCase();
    if (match && TYPES.includes(type)) {
      if (!blocks.sections.has(type)) blocks.sections.set(type, []);
      current = blocks.sections.get(type);
    } else current.push(line);
  }
  return blocks;
}

const trimBlank = (lines) => {
  const out = [...lines];
  while (out.length && !out[0].trim()) out.shift();
  while (out.length && !out.at(-1).trim()) out.pop();
  return out;
};

function renderSection({ preamble, sections }) {
  const parts = [];
  const intro = trimBlank(preamble);
  if (intro.length) parts.push(intro.join("\n"));
  for (const type of TYPES) {
    const lines = trimBlank(sections.get(type) || []);
    if (lines.length) parts.push(`${heading(type)}\n\n${lines.join("\n")}`);
  }
  return parts.join("\n\n");
}

function replaceUnreleased(text, render) {
  const { start, end } = unreleasedBounds(text);
  const body = render(text.slice(start, end));
  const rest = text.slice(end);
  return `${text.slice(0, start)}\n\n${body ? `${body}\n\n` : ""}${rest}`.replace(
    /\n{3,}$/,
    "\n",
  );
}

// Complete bullet blocks: a "- " line plus its continuation lines.
function bullets(lines) {
  const out = [];
  for (const line of lines) {
    if (line.startsWith("- ") || out.length === 0) out.push(line);
    else if (line.trim()) out[out.length - 1] += `\n${line}`;
  }
  return out;
}

// Pure: merge changesets into the changelog text. Already-present entries are skipped.
export function applyChangesets(changelog, changesets) {
  const source = lf(changelog);
  return replaceUnreleased(source, (content) => {
    const blocks = parseSection(content);
    for (const changeset of changesets) {
      const entry = renderEntry(changeset).split("\n");
      const lines = trimBlank(blocks.sections.get(changeset.type) || []);
      const present = new Set(bullets(lines));
      if (bullets(entry).every((bullet) => present.has(bullet))) continue;
      blocks.sections.set(changeset.type, [...lines, ...entry]);
    }
    return renderSection(blocks);
  });
}

export function consume(root = ".") {
  const changesets = readChangesets(root);
  if (changesets.length === 0) return [];
  const path = join(root, CHANGELOG);
  const before = readFileSync(path, "utf8");
  const after = applyChangesets(before, changesets);
  if (after !== lf(before)) writeFileSync(path, after);
  for (const { name } of changesets) rmSync(join(root, CHANGES_DIR, name));
  return changesets.map(({ name }) => name);
}

export function moveUnreleased(changelog, version, date, summary = "") {
  if (!VERSION.test(version))
    throw new ChangesetError(`invalid version "${version}"`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date))
    throw new ChangesetError(`invalid date "${date}"`);
  const source = lf(changelog);
  if (
    new RegExp(`^## \\[?${version.replace(/\./g, "\\.")}\\]?\\b`, "m").test(
      source,
    )
  )
    throw new ChangesetError(`${CHANGELOG}: ${version} already exists`);
  const { head, start, end } = unreleasedBounds(source);
  const content = source.slice(start, end).trim();
  if (!content) throw new ChangesetError(`${CHANGELOG}: Unreleased is empty`);
  const intro = summary.trim() ? `${summary.trim()}\n\n` : "";
  const unreleased = source.slice(head, start);
  return `${source.slice(0, head)}${unreleased}\n\n## ${version} - ${date}\n\n${intro}${content}\n\n${source.slice(end)}`.replace(
    /\n{3,}$/,
    "\n",
  );
}

// Every checked-in version field from docs/releases.md "Cut a release".
export const VERSION_FIELDS = [
  ["package.json", (v) => [/^(  "version": ")[^"]+(")/m, `$1${v}$2`]],
  ["package-lock.json", (v) => [/^(  "version": ")[^"]+(")/m, `$1${v}$2`]],
  [
    "package-lock.json",
    (v) => [
      /^(    "": \{\n(?:      .*\n)*?      "version": ")[^"]+(")/m,
      `$1${v}$2`,
    ],
  ],
  [
    "src-tauri/tauri.conf.json",
    (v) => [/^(  "version": ")[^"]+(")/m, `$1${v}$2`],
  ],
  [
    "src-tauri/Cargo.toml",
    (v) => [/^(\[package\]\n(?:.*\n)*?version = ")[^"]+(")/m, `$1${v}$2`],
  ],
  [
    "src-tauri/Cargo.lock",
    (v) => [/^(name = "pr-sniper"\nversion = ")[^"]+(")/m, `$1${v}$2`],
  ],
  ["site/index.html", (v) => [/(id="version">v)[^<]+(<)/, `$1${v}$2`]],
  ["site/index.html", (v) => [/(id="hero-version">v)[^<]+(<)/, `$1${v}$2`]],
  ["site/app.js", (v) => [/("TERMLINK BIOS )[^ ]+( ·)/, `$1${v}$2`]],
];

export function currentVersion(root = ".") {
  return JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
}

const newer = (a, b) => {
  const [x, y] = [a, b].map((v) => v.split(".").map(Number));
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i];
  return false;
};

export function bumpVersions(root, version) {
  for (const [file, rule] of VERSION_FIELDS) {
    const path = join(root, file);
    const text = lf(readFileSync(path, "utf8"));
    const [pattern, replacement] = rule(version);
    if (!pattern.test(text))
      throw new ChangesetError(`${file}: version field not found`);
    writeFileSync(path, text.replace(pattern, replacement));
  }
  const lock = JSON.parse(
    readFileSync(join(root, "package-lock.json"), "utf8"),
  );
  if (lock.version !== version || lock.packages[""].version !== version)
    throw new ChangesetError("package-lock.json: version readback mismatch");
}

export function release(root, version, { date, summary = "", build } = {}) {
  if (!VERSION.test(version))
    throw new ChangesetError(`invalid version "${version}"`);
  const previous = currentVersion(root);
  if (!newer(version, previous))
    throw new ChangesetError(`${version} must be newer than ${previous}`);
  const consumed = consume(root);
  const path = join(root, CHANGELOG);
  const day = date || new Date().toISOString().slice(0, 10);
  writeFileSync(
    path,
    moveUnreleased(readFileSync(path, "utf8"), version, day, summary),
  );
  bumpVersions(root, version);
  (build ?? defaultBuild)(root);
  return { version, previous, date: day, consumed };
}

function defaultBuild(root) {
  execFileSync("npm", ["run", "build:control-mcp"], {
    cwd: root,
    stdio: "inherit",
    shell: process.platform === "win32",
  });
}

// PR policy. `changed` is [{status, path}] from `git diff --name-status base...head`.
export function validate({
  root = ".",
  changed = [],
  labels = [],
  headRef = "",
  author = "",
  sameRepo = true,
} = {}) {
  const errors = [];
  const warnings = [];
  try {
    readChangesets(root);
  } catch (error) {
    if (!(error instanceof ChangesetError)) throw error;
    errors.push(error.message);
  }
  const bot =
    BOT_AUTHORS.includes(author) ||
    (sameRepo && headRef.startsWith(BOT_BRANCH_PREFIX));
  const optOut = labels.includes(OPT_OUT_LABEL);
  if (changed.some(({ path }) => path === CHANGELOG) && !bot && !optOut)
    errors.push(
      `${CHANGELOG} edited directly; add ${CHANGES_DIR}/<name>.md instead (or label ${OPT_OUT_LABEL})`,
    );
  const added = changed.some(
    ({ status, path }) =>
      /^[AMR]/.test(status) &&
      path.startsWith(`${CHANGES_DIR}/`) &&
      !RESERVED.has(path.slice(CHANGES_DIR.length + 1)),
  );
  if (changed.length && !added && !bot && !optOut)
    warnings.push(
      `no ${CHANGES_DIR}/*.md entry; add one for notable changes or label ${OPT_OUT_LABEL}`,
    );
  return { errors, warnings };
}

function args(argv) {
  const options = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (!arg.startsWith("--")) options._.push(arg);
    else if (arg === "--skip-build") options.skipBuild = true;
    else options[arg.slice(2)] = argv[++i] ?? "";
  }
  return options;
}

function diff(root, base, head) {
  const output = execFileSync(
    "git",
    ["-C", root, "diff", "--name-status", "--no-renames", `${base}...${head}`],
    { encoding: "utf8" },
  );
  return output
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const [status, path] = line.split("\t");
      return { status, path };
    });
}

export function main(argv = process.argv.slice(2)) {
  const options = args(argv);
  const [command, value] = options._;
  const root = options.root || ".";
  if (command === "validate") {
    const changed = options.base
      ? diff(root, options.base, options.head || "HEAD")
      : [];
    const { errors, warnings } = validate({
      root,
      changed,
      labels: (options.labels || "").split(",").map((s) => s.trim()),
      headRef: options["head-ref"] || "",
      author: options.author || "",
      sameRepo: options["same-repo"] !== "false",
    });
    for (const warning of warnings) console.log(`::warning::${warning}`);
    for (const error of errors) console.log(`::error::${error}`);
    if (errors.length) return 1;
    console.log(`changesets: ${listChangesets(root).length} valid`);
    return 0;
  }
  if (command === "list") {
    readChangesets(root);
    for (const name of listChangesets(root)) console.log(name);
    return 0;
  }
  if (command === "consume") {
    const consumed = consume(root);
    console.log(
      consumed.length ? `consumed: ${consumed.join(", ")}` : "consumed: none",
    );
    return 0;
  }
  if (command === "release") {
    const result = release(root, value || "", {
      date: options.date,
      summary: options.summary || "",
      build: options.skipBuild ? () => {} : undefined,
    });
    console.log(JSON.stringify(result));
    return 0;
  }
  console.error(
    "usage: changesets.mjs validate [--base SHA --head SHA --labels a,b --head-ref REF --author LOGIN --same-repo true|false] | list | consume | release X.Y.Z [--date YYYY-MM-DD] [--summary TEXT] [--skip-build]",
  );
  return 2;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main();
  } catch (error) {
    if (!(error instanceof ChangesetError)) throw error;
    console.error(`::error::${error.message}`);
    process.exitCode = 1;
  }
}
