import { createHash } from 'node:crypto';
import { closeSync, constants, fstatSync, lstatSync, openSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const DEFAULT_ROOT = fileURLToPath(new URL('../doctrines/', import.meta.url));
const ID = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;
const HASH = /^[a-f0-9]{64}$/;

function fail(message) {
  throw new Error(message);
}

function canonicalId(value) {
  if (typeof value !== 'string' || !ID.test(value)) fail(`Invalid doctrine ID: ${value}`);
  return value === 'lazy' ? 'laziness' : value;
}

function checkPath(filename, kind) {
  const absolute = path.resolve(filename);
  const { root } = path.parse(absolute);
  let current = root;
  for (const component of absolute.slice(root.length).split(path.sep).filter(Boolean)) {
    current = path.join(current, component);
    const stat = lstatSync(current);
    if (stat.isSymbolicLink()) fail(`Symlink is not allowed: ${current}`);
    const isLast = current === absolute;
    if ((!isLast || kind === 'directory') && !stat.isDirectory()) {
      fail(`Expected directory: ${current}`);
    }
    if (isLast && kind === 'file' && !stat.isFile()) fail(`Expected regular file: ${current}`);
  }
  return absolute;
}

function readText(filename) {
  checkPath(filename, 'file');
  const descriptor = openSync(filename, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    if (!fstatSync(descriptor).isFile()) fail(`Expected regular file: ${filename}`);
    const bytes = readFileSync(descriptor);
    let text;
    try {
      text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
    } catch {
      fail(`Invalid UTF-8: ${filename}`);
    }
    return { text, sha256: createHash('sha256').update(bytes).digest('hex') };
  } finally {
    closeSync(descriptor);
  }
}

function frontmatter(text, label) {
  const lines = text.split(/\r?\n/);
  if (lines[0] !== '---') fail(`Missing frontmatter: ${label}`);
  const end = lines.indexOf('---', 1);
  if (end === -1) fail(`Unclosed frontmatter: ${label}`);
  return lines.slice(1, end);
}

function parseManifest(text) {
  const lines = frontmatter(text, 'manifest');
  if (lines[0] !== 'schema-version: 1') fail('Unsupported or malformed manifest schema-version');
  if (lines[1] !== 'doctrine:') fail('Malformed manifest: expected doctrine list');
  const entries = [];
  const ids = new Set();
  for (let index = 2; index < lines.length; index += 3) {
    const id = /^  - id: ([a-z][a-z0-9]*(?:-[a-z0-9]+)*)$/.exec(lines[index])?.[1];
    const sourcePath = /^    path: ([^\s]+)$/.exec(lines[index + 1] ?? '')?.[1];
    const sha256 = /^    sha256: ([a-f0-9]{64})$/.exec(lines[index + 2] ?? '')?.[1];
    if (!id || !sourcePath || !sha256) fail(`Malformed manifest entry at line ${index + 2}`);
    if (id === 'lazy') fail('Manifest ID lazy is reserved as an alias for laziness');
    if (ids.has(id)) fail(`Duplicate manifest ID: ${id}`);
    const segments = sourcePath.split('/');
    if (segments.some(segment => !/^[a-zA-Z0-9_.-]+$/.test(segment) || segment === '.' || segment === '..')) {
      fail(`Unsafe manifest path for ${id}: ${sourcePath}`);
    }
    if (segments.at(-1) !== `${id}.doctrine.md`) fail(`Filename does not match doctrine ID: ${id}`);
    ids.add(id);
    entries.push({ id, relativePath: sourcePath, sha256 });
  }
  if (entries.length === 0) fail('Manifest contains no doctrines');
  return entries;
}

function scalar(raw, label) {
  let value;
  if (raw.startsWith('"')) {
    // JSON strings are the supported double-quoted subset; never evaluate YAML.
    try {
      value = JSON.parse(raw);
    } catch {
      fail(`Invalid quoted scalar: ${label}`);
    }
  } else if (raw.startsWith("'")) {
    if (!/^'(?:[^']|'')*'$/.test(raw)) fail(`Invalid quoted scalar: ${label}`);
    value = raw.slice(1, -1).replaceAll("''", "'");
  } else {
    if (/^[!&*|>{}\[\],%@`?#:-]/.test(raw) || /:\s|\s#/.test(raw) ||
        /^(?:null|true|false|~|[-+]?\d+(?:\.\d+)?)$/i.test(raw)) {
      fail(`Unsupported plain scalar: ${label}`);
    }
    value = raw;
  }
  if (typeof value !== 'string' || !value.trim() || /[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(value)) {
    fail(`Expected nonempty single-line scalar: ${label}`);
  }
  return value;
}

function sourceDescription(text, id) {
  const fields = new Map();
  for (const line of frontmatter(text, id)) {
    const match = /^(name|description|scope): (.+)$/.exec(line);
    if (!match || fields.has(match[1])) fail(`Malformed or duplicate source frontmatter field: ${id}`);
    fields.set(match[1], scalar(match[2], `${id}.${match[1]}`));
  }
  if (fields.get('name') !== id) fail(`Source name does not match doctrine ID: ${id}`);
  if (!fields.has('description')) fail(`Missing source description: ${id}`);
  return fields.get('description');
}

function verifiedSources(root) {
  const directory = checkPath(root, 'directory');
  const manifest = readText(path.join(directory, 'manifest.md'));
  return parseManifest(manifest.text).map(entry => {
    const filename = path.resolve(directory, entry.relativePath);
    if (!filename.startsWith(`${directory}${path.sep}`)) fail(`Path escapes doctrines root: ${entry.id}`);
    const source = readText(filename);
    if (source.sha256 !== entry.sha256) fail(`SHA-256 mismatch for ${entry.id}: ${filename}`);
    return {
      id: entry.id,
      description: sourceDescription(source.text, entry.id),
      path: filename,
      sha256: source.sha256,
      text: source.text,
    };
  });
}

/** Parse only explicit choices; required IDs participate in first-seen order. */
export function parseArgs(args) {
  const choices = new Map();
  const expected = new Map();
  let mode;
  for (let index = 0; index < args.length; index++) {
    const argument = args[index];
    if (argument === '--select' || argument === '--list') {
      if (mode) fail('Duplicate or conflicting mode flags');
      mode = argument === '--select' ? 'selection' : 'catalog';
    } else if (argument === '--expect') {
      const value = args[++index];
      const match = /^([^=]+)=([a-fA-F0-9]{64})$/.exec(value ?? '');
      if (!match) fail('Expected --expect ID=SHA256 with a 64-digit hexadecimal hash');
      const id = canonicalId(match[1]);
      const hash = match[2].toLowerCase();
      if (!HASH.test(hash)) fail(`Invalid expected hash for ${id}`);
      if (expected.has(id) && expected.get(id) !== hash) fail(`Conflicting expected hashes for ${id}`);
      expected.set(id, hash);
    } else {
      const required = argument === '--required';
      if (!required && argument.startsWith('-')) fail(`Unknown option: ${argument}`);
      const value = required ? args[++index] : argument;
      if (required && (!value || value.startsWith('-'))) fail('Missing ID after --required');
      const id = canonicalId(value);
      choices.set(id, required || choices.get(id) === true);
    }
  }
  if (mode === 'catalog' && (choices.size || expected.size)) fail('--list cannot be combined with selections or expectations');
  for (const id of expected.keys()) {
    if (!choices.has(id)) fail(`Expected hash ID is not selected: ${id}`);
  }
  return {
    mode: choices.size ? (mode ?? 'load') : 'catalog',
    choices,
    expected,
  };
}

/** Read and validate the complete bundle before returning any result. Root is API-only. */
export function run(args = [], root = DEFAULT_ROOT) {
  const { mode, choices, expected } = parseArgs(args);
  const sources = verifiedSources(root);
  if (mode === 'catalog') {
    return { mode, doctrines: sources.map(({ id, description }) => ({ id, description })) };
  }
  const byId = new Map(sources.map(source => [source.id, source]));
  const doctrines = [...choices].map(([id, required]) => {
    const source = byId.get(id);
    if (!source) fail(`Unknown ${required ? 'required' : 'optional'} doctrine ID: ${id}`);
    if (expected.has(id) && expected.get(id) !== source.sha256) fail(`Pinned SHA-256 mismatch for ${id}`);
    const { text, ...metadata } = source;
    return mode === 'load' ? { ...metadata, required, text } : { ...metadata, required };
  });
  return { mode, doctrines };
}

export function formatResult(result) {
  if (result.mode !== 'load') return `${JSON.stringify(result)}\n`;
  return result.doctrines.map(source =>
    `=== doctrine ${source.id} ===\npath: ${source.path}\nsha256: ${source.sha256}\nrequired: ${source.required}\n--- source ---\n${source.text}\n=== end doctrine ${source.id} ===\n`,
  ).join('\n');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.stdout.write(formatResult(run(process.argv.slice(2))));
  } catch (error) {
    process.stderr.write(`doctrine: ${error.message}\n`);
    process.exitCode = 1;
  }
}
