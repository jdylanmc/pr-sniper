import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { DEFAULT_ROOT, formatResult, parseArgs, run } from '../scripts/doctrine.mjs';

const HELPER = fileURLToPath(new URL('../scripts/doctrine.mjs', import.meta.url));
const TEST_ROOT = fileURLToPath(new URL('../../../../.test-sandbox/', import.meta.url));
const CODE = '---\nname: code\ndescription: "Readable code."\nscope: shared-engineering-doctrine\n---\n\n# Code\nKeep BODY_CODE_SECRET exact.  \n';
const CODE_HASH = '2fa3b19d32672a8db5ad4aad921126d995838806d7fcd83d5e13a61fd5fa798d';
const LAZY = "---\r\nname: laziness\r\ndescription: 'Do less; don''t guess.'\r\n---\r\nBODY_LAZY_SECRET";
const LAZY_HASH = 'bef08d8bd4bcc17bf2c234aa4c767f1345d8a65f2e1d40f98fc5a3f1ad78e031';
const MANIFEST = `---
schema-version: 1
doctrine:
  - id: code
    path: code.doctrine.md
    sha256: ${CODE_HASH}
  - id: laziness
    path: laziness.doctrine.md
    sha256: ${LAZY_HASH}
---

# Documentation, not manifest entries
  - id: ignored
    path: outside.doctrine.md
    sha256: invalid
`;

function fixture(t) {
  // Keep transient fixtures outside the package that distribution tests copy.
  mkdirSync(TEST_ROOT, { recursive: true });
  const base = path.join(TEST_ROOT, `.fixture-${randomUUID()}`);
  mkdirSync(base);
  t.after(() => rmSync(base, { recursive: true, force: true }));
  const root = path.join(base, 'doctrines');
  const scripts = path.join(base, 'scripts');
  mkdirSync(root);
  mkdirSync(scripts);
  mkdirSync(path.join(base, 'unrelated'));
  writeFileSync(path.join(root, 'manifest.md'), MANIFEST);
  writeFileSync(path.join(root, 'code.doctrine.md'), CODE);
  writeFileSync(path.join(root, 'laziness.doctrine.md'), LAZY);
  const cli = path.join(scripts, 'doctrine.mjs');
  copyFileSync(HELPER, cli);
  return { base, root, cli, cwd: path.join(base, 'unrelated') };
}

function changeManifest(root, transform) {
  const filename = path.join(root, 'manifest.md');
  writeFileSync(filename, transform(readFileSync(filename, 'utf8')));
}

function replaceSource(root, text) {
  writeFileSync(path.join(root, 'code.doctrine.md'), text);
  const hash = createHash('sha256').update(text).digest('hex');
  changeManifest(root, value => value.replace(CODE_HASH, hash));
}

function cliFailure(f, args, expected) {
  const result = spawnSync(process.execPath, [f.cli, ...args], { cwd: f.cwd, encoding: 'utf8' });
  assert.ifError(result.error);
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.match(result.stderr, expected);
}

function makeSymlink(t, target, link, type) {
  try {
    symlinkSync(target, link, type);
    return true;
  } catch (error) {
    if (!['EPERM', 'EACCES', 'ENOSYS'].includes(error.code)) throw error;
    t.skip(`Symlinks unavailable: ${error.code}`);
    return false;
  }
}

test('catalog exposes only verified canonical IDs and descriptions, not source bodies', t => {
  const { root } = fixture(t);
  const expected = {
    mode: 'catalog',
    doctrines: [
      { id: 'code', description: 'Readable code.' },
      { id: 'laziness', description: "Do less; don't guess." },
    ],
  };
  assert.deepEqual(run([], root), expected);
  assert.deepEqual(run(['--list'], root), expected);
  assert.deepEqual(run(['--select'], root), expected);
  assert.doesNotMatch(formatResult(expected), /BODY_|sha256|\.doctrine\.md/);
});

test('metadata selections deduplicate aliases, preserve encounter order, and union required IDs', t => {
  const { root } = fixture(t);
  const result = run(['--select', 'lazy', 'code', '--required', 'laziness', '--required', 'code', 'lazy'], root);
  assert.deepEqual(result, {
    mode: 'selection',
    doctrines: [
      { id: 'laziness', description: "Do less; don't guess.", path: path.join(root, 'laziness.doctrine.md'), sha256: LAZY_HASH, required: true },
      { id: 'code', description: 'Readable code.', path: path.join(root, 'code.doctrine.md'), sha256: CODE_HASH, required: true },
    ],
  });
  assert.doesNotMatch(formatResult(result), /BODY_|--- source ---/);
  assert.deepEqual(run(['--select', '--required', 'code', 'lazy'], root).doctrines.map(({ id, required }) => ({ id, required })), [
    { id: 'code', required: true }, { id: 'laziness', required: false },
  ]);
  assert.deepEqual(run(['--select', '--required', 'lazy'], root).doctrines.map(source => source.id), ['laziness']);
});

test('named loads preserve complete literal source text, CRLF, trailing spaces, and absent final newline', t => {
  const { root } = fixture(t);
  const result = run(['code', 'lazy', 'code'], root);
  assert.equal(result.mode, 'load');
  assert.deepEqual(result.doctrines.map(({ id, sha256, required, text }) => ({ id, sha256, required, text })), [
    { id: 'code', sha256: CODE_HASH, required: false, text: CODE },
    { id: 'laziness', sha256: LAZY_HASH, required: false, text: LAZY },
  ]);
  assert.equal(formatResult(run(['code'], root)),
    `=== doctrine code ===\npath: ${path.join(root, 'code.doctrine.md')}\nsha256: ${CODE_HASH}\nrequired: false\n--- source ---\n${CODE}\n=== end doctrine code ===\n`);
  assert.equal(run(['--required', 'lazy'], root).doctrines[0].required, true);
});

test('pinned expectations work for metadata and loads, including equivalent alias pins', t => {
  const { root } = fixture(t);
  for (const mode of [[], ['--select']]) {
    const result = run([...mode, 'lazy', '--expect', `lazy=${LAZY_HASH.toUpperCase()}`, '--expect', `laziness=${LAZY_HASH}`], root);
    assert.equal(result.doctrines[0].sha256, LAZY_HASH);
    assert.throws(() => run([...mode, 'lazy', '--expect', `laziness=${CODE_HASH}`], root), /Pinned SHA-256 mismatch for laziness/);
  }
});

test('unknown optional and required IDs are explicit failures', t => {
  const { root } = fixture(t);
  assert.throws(() => run(['code', 'unknown'], root), /Unknown optional doctrine ID: unknown/);
  assert.throws(() => run(['--select', '--required', 'unknown'], root), /Unknown required doctrine ID: unknown/);
});

test('plain and quoted descriptions are parsed as inert single-line data', t => {
  const { root } = fixture(t);
  for (const [raw, expected] of [
    ['Read code; do not execute $(anything).', 'Read code; do not execute $(anything).'],
    ['"Use \\"quotes\\" safely: # literal."', 'Use "quotes" safely: # literal.'],
    ["'Do not ''evaluate'' this.'", "Do not 'evaluate' this."],
  ]) {
    replaceSource(root, CODE.replace('"Readable code."', raw));
    assert.equal(run([], root).doctrines[0].description, expected);
    // Restore the baseline so each replacement updates the manifest's correct hash.
    writeFileSync(path.join(root, 'manifest.md'), MANIFEST);
  }
});

for (const [name, transform, error] of [
  ['unsupported schema', value => value.replace('schema-version: 1', 'schema-version: 2'), /schema-version/],
  ['missing opening delimiter', value => value.slice(4), /frontmatter/],
  ['unclosed frontmatter', value => value.replace('\n---\n\n#', '\n\n#'), /Unclosed/],
  ['empty manifest', () => '---\nschema-version: 1\ndoctrine:\n---\n', /no doctrines/],
  ['malformed later entry', value => value.replace('  - id: laziness', '  - id laziness'), /Malformed manifest entry/],
  ['missing digest', value => value.replace(`    sha256: ${LAZY_HASH}\n`, ''), /Malformed manifest entry/],
  ['bad digest', value => value.replace(LAZY_HASH, 'xyz'), /Malformed manifest entry/],
  ['bad canonical ID', value => value.replace('id: laziness', 'id: Not_Canonical'), /Malformed manifest entry/],
  ['duplicate ID', value => value.replace('id: laziness', 'id: code'), /Duplicate manifest ID/],
  ['reserved alias ID', value => value.replace('id: laziness', 'id: lazy'), /reserved/],
  ['filename mismatch', value => value.replace('path: code.doctrine.md', 'path: other.doctrine.md'), /Filename/],
  ['parent escape', value => value.replace('path: code.doctrine.md', 'path: ../code.doctrine.md'), /Unsafe manifest path/],
  ['absolute escape', value => value.replace('path: code.doctrine.md', 'path: /code.doctrine.md'), /Unsafe manifest path/],
  ['Windows absolute path', value => value.replace('path: code.doctrine.md', 'path: C:\\code.doctrine.md'), /Unsafe manifest path/],
  ['dot segment', value => value.replace('path: code.doctrine.md', 'path: ./code.doctrine.md'), /Unsafe manifest path/],
  ['unknown manifest key', value => value.replace('doctrine:\n', 'extra: ignored\ndoctrine:\n'), /Malformed manifest/],
  ['extra entry field', value => value.replace(`sha256: ${CODE_HASH}\n`, `sha256: ${CODE_HASH}\n    ignored: true\n`), /Malformed manifest entry/],
  ['duplicate entry field', value => value.replace('    path: code.doctrine.md\n', '    path: code.doctrine.md\n    path: code.doctrine.md\n'), /Malformed manifest entry/],
  ['unparsed trailing header line', value => value.replace('\n---\n\n#', '\ngarbage\n---\n\n#'), /Malformed manifest entry/],
]) {
  test(`rejects ${name} instead of partially accepting the manifest`, t => {
    const f = fixture(t);
    changeManifest(f.root, transform);
    assert.throws(() => run([], f.root), error);
    cliFailure(f, ['code'], error);
  });
}

for (const [name, transform, error] of [
  ['wrong name', value => value.replace('name: code', 'name: machine'), /Source name/],
  ['missing description', value => value.replace('description: "Readable code."\n', ''), /Missing source description/],
  ['duplicate description', value => value.replace('description:', 'description: "First."\ndescription:'), /duplicate source frontmatter/],
  ['unknown field', value => value.replace('scope:', 'other:'), /Malformed/],
  ['empty description', value => value.replace('"Readable code."', '""'), /nonempty single-line/],
  ['multiline scalar', value => value.replace('"Readable code."', '>\n  Folded text'), /Unsupported plain scalar/],
  ['collection scalar', value => value.replace('"Readable code."', '[one, two]'), /Unsupported plain scalar/],
  ['anchor scalar', value => value.replace('"Readable code."', '&alias text'), /Unsupported plain scalar/],
  ['unclosed quote', value => value.replace('"Readable code."', '"Unclosed'), /Invalid quoted scalar/],
  ['newline escape', value => value.replace('"Readable code."', '"Line\\nBreak"'), /single-line/],
  ['plain inline comment', value => value.replace('"Readable code."', 'Words # ignored'), /Unsupported plain scalar/],
  ['plain mapping', value => value.replace('"Readable code."', 'Words: ignored'), /Unsupported plain scalar/],
]) {
  test(`rejects ${name} even with a valid source hash`, t => {
    const { root } = fixture(t);
    replaceSource(root, transform(CODE));
    assert.throws(() => run([], root), error);
  });
}

test('missing sources and manifest, tampering, and file/directory confusion fail closed', t => {
  const f = fixture(t);
  const source = path.join(f.root, 'laziness.doctrine.md');
  writeFileSync(source, `${LAZY}\nTampered`);
  assert.throws(() => run([], f.root), /SHA-256 mismatch for laziness/);
  cliFailure(f, ['code', 'lazy'], /SHA-256 mismatch for laziness/);
  rmSync(source);
  assert.throws(() => run(['--select', 'code'], f.root), /ENOENT/);
  mkdirSync(source);
  assert.throws(() => run([], f.root), /Expected regular file/);
  rmSync(path.join(f.root, 'manifest.md'));
  assert.throws(() => run([], f.root), /ENOENT/);
  mkdirSync(path.join(f.root, 'manifest.md'));
  assert.throws(() => run([], f.root), /Expected regular file/);
  assert.throws(() => run([], path.join(f.root, 'code.doctrine.md')), /Expected directory/);
});

test('invalid UTF-8 is rejected rather than replacing source bytes', t => {
  const { root } = fixture(t);
  const bytes = Buffer.concat([Buffer.from(CODE), Buffer.from([0xff])]);
  writeFileSync(path.join(root, 'code.doctrine.md'), bytes);
  changeManifest(root, value => value.replace(CODE_HASH, createHash('sha256').update(bytes).digest('hex')));
  assert.throws(() => run(['code'], root), /Invalid UTF-8/);
});

test('safe nested source paths are supported', t => {
  const { root } = fixture(t);
  mkdirSync(path.join(root, 'nested'));
  renameSync(path.join(root, 'code.doctrine.md'), path.join(root, 'nested/code.doctrine.md'));
  changeManifest(root, value => value.replace('path: code.doctrine.md', 'path: nested/code.doctrine.md'));
  const source = run(['code'], root).doctrines[0];
  assert.equal(source.path, path.join(root, 'nested/code.doctrine.md'));
  assert.equal(source.text, CODE);
});

for (const component of ['source', 'manifest', 'root', 'ancestor', 'source ancestor']) {
  test(`rejects symlink at ${component}, even pointing to valid in-bundle content`, t => {
    const f = fixture(t);
    let root = f.root;
    if (component === 'source' || component === 'manifest') {
      const filename = path.join(root, component === 'source' ? 'code.doctrine.md' : 'manifest.md');
      renameSync(filename, `${filename}.real`);
      if (!makeSymlink(t, `${filename}.real`, filename, 'file')) return;
    } else if (component === 'root') {
      root = path.join(f.base, 'linked-root');
      if (!makeSymlink(t, f.root, root, 'dir')) return;
    } else if (component === 'ancestor') {
      const linked = path.join(f.base, 'linked-parent');
      if (!makeSymlink(t, f.base, linked, 'dir')) return;
      root = path.join(linked, 'doctrines');
    } else {
      if (!makeSymlink(t, root, path.join(root, 'linked'), 'dir')) return;
      changeManifest(root, value => value.replace('path: code.doctrine.md', 'path: linked/code.doctrine.md'));
    }
    assert.throws(() => run(['code'], root), /Symlink is not allowed/);
  });
}

for (const args of [
  ['--unknown'], ['--root', '/arbitrary'], ['--'], ['--select', '--select'],
  ['--list', '--list'], ['--list', '--select'], ['--list', 'code'],
  ['--list', '--required', 'code'], ['--required'], ['--required', '--select'],
  ['--expect'], ['--expect', 'code=bad'], ['--expect', `code=${'g'.repeat(64)}`],
  ['--expect', `code=${CODE_HASH}`], ['--select', '--expect', `code=${CODE_HASH}`],
  ['code', '--expect', `lazy=${LAZY_HASH}`], ['code', '--expect', `code=${CODE_HASH}`, '--expect', `code=${LAZY_HASH}`],
  ['lazy', '--expect', `lazy=${CODE_HASH}`, '--expect', `laziness=${LAZY_HASH}`],
  ['Code'], ['code,lazy'], [''], ['../code'], ['code', '--expect=code=bad'],
]) {
  test(`malformed CLI arguments fail without stdout: ${JSON.stringify(args)}`, t => {
    const f = fixture(t);
    assert.throws(() => parseArgs(args));
    cliFailure(f, args, /^doctrine: .+/);
  });
}

test('CLI validates late unknown names and pin failures before emitting any selected text', t => {
  const f = fixture(t);
  cliFailure(f, ['code', 'unknown'], /Unknown optional/);
  cliFailure(f, ['code', '--required', 'unknown'], /Unknown required/);
  cliFailure(f, ['code', 'lazy', '--expect', `lazy=${CODE_HASH}`], /Pinned SHA-256 mismatch/);
  cliFailure(f, ['--select', 'code', 'unknown'], /Unknown optional/);
});

test('actual CLI uses its bundled root from unrelated cwd and emits exact framed sources', t => {
  const f = fixture(t);
  const invoke = args => execFileSync(process.execPath, [f.cli, ...args], { cwd: f.cwd, encoding: 'utf8' });
  assert.deepEqual(JSON.parse(invoke([])), run([], f.root));
  const metadata = invoke(['--select', 'lazy', '--required', 'code']);
  assert.equal(JSON.parse(metadata).doctrines[0].sha256, LAZY_HASH);
  assert.doesNotMatch(metadata, /BODY_/);
  assert.equal(invoke(['code', 'lazy']), formatResult(run(['code', 'lazy'], f.root)));
  assert.equal(invoke(['code', '--expect', `code=${CODE_HASH}`]), formatResult(run(['code'], f.root)));
  const catalog = JSON.parse(execFileSync(process.execPath, [HELPER, '--list'], { cwd: f.cwd, encoding: 'utf8' }));
  assert.deepEqual(catalog, run([], DEFAULT_ROOT));
  assert.ok(catalog.doctrines.some(({ id }) => id === 'code'));
  assert.ok(catalog.doctrines.some(({ id }) => id === 'laziness'));
});

test('importing the helper does not read doctrine files or emit output', t => {
  const f = fixture(t);
  rmSync(f.root, { recursive: true });
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `await import(${JSON.stringify(pathToFileURL(f.cli).href)})`], {
    cwd: f.cwd, encoding: 'utf8',
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, '');
  assert.equal(result.stderr, '');
});
