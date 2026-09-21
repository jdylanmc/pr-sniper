import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const read = file => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');
const guide = read('TRANSPORT.md');
const normalized = text => text.replace(/\s+/g, ' ');
function section(heading) {
  const start = guide.indexOf(`## ${heading}\n`);
  assert.notEqual(start, -1, heading);
  const end = guide.indexOf('\n## ', start + 1);
  return normalized(guide.slice(start, end === -1 ? undefined : end));
}

test('transport prose contract is supporting guidance reachable from all adapter gates', () => {
  assert.doesNotMatch(guide, /^---\n/);
  assert.match(guide, /joe-mode-paseo only/);
  for (const file of ['SKILL.md', 'RUNTIME.md', 'STATE.md', 'RUN.md', 'SCENARIOS.md']) {
    const text = read(file);
    assert.match(text, /\[[^\]]+\]\(TRANSPORT\.md(?:#[^)]+)?\)/, file);
    assert.doesNotMatch(text, /Use only MCP create\/delete|uses MCP \*\*create\/delete only\*\*|deletes its owned heartbeat through MCP and/);
  }
  assert.match(read('SCENARIOS.md'), /not live transport behavior/);
  assert.match(normalized(read('SCENARIOS.md')), /remain unverified until separately authorized observations/);
});

test('diagnosis prose distinguishes reachability, exposure, policy, permission and progress', () => {
  const claims = section('Separate the claims');
  for (const layer of ['Daemon reachability and caller', 'Server-advertised catalog',
    'Harness-discovered tools', 'Intentional provider policy', 'Provider approval', 'OS permission']) {
    assert.ok(claims.includes(layer), layer);
  }
  assert.match(claims, /Available \/ authorized \/ requested \/ accepted \/ completed \/ verified/);
  assert.match(claims, /visible Screen Recording\/Accessibility setting is not effective access/);
  assert.match(claims, /none grants OS access/);
  assert.match(claims, /earlier false preflight is not a post-grant check/);
});

test('diagnostic ladder is ordered, bounded and keeps credentials and hidden tools out', () => {
  const ladder = section('One bounded read-only ladder');
  const steps = [...ladder.matchAll(/\b([1-6])\. \*\*([^*]+)\*\*/g)];
  assert.deepEqual(steps.map(step => step[1]), ['1', '2', '3', '4', '5', '6']);
  assert.match(ladder, /deferred-tool discovery/);
  assert.match(ladder, /paseoTools\.enabled.*disabledTools.*daemon\.mcp\.injectIntoAgents/);
  assert.match(ladder, /authenticated `initialize` and `tools\/list`.*same existing connection configuration/);
  assert.match(ladder, /Bound pagination.*close the diagnostic session/);
  assert.match(ladder, /No `tools\/call`.*no ad hoc general raw RPC client/);
  assert.match(ladder, /credentials in memory, never print them/);
  assert.match(ladder, /Never search unrelated agents or files for another token/);
  assert.match(ladder, /catalog-exposure mismatch.*not daemon outage/);
  assert.match(ladder, /Do not repeat the ladder for an unchanged episode/);
  assert.match(ladder, /No full transcripts or broad environment\/config dumps/);
});

test('CLI selection prose requires provenance and binding without installation repair', () => {
  const cli = section('Resolve the CLI without changing installations');
  assert.match(cli, /Before executing even version\/help, establish executable provenance/);
  assert.match(cli, /aliases, wrappers and symlinks/);
  assert.match(cli, /PASEO_CLI.*Desktop-bundled/);
  assert.match(cli, /absolute executable, version, execution host, daemon host\/home and original caller binding/);
  assert.match(cli, /installed `--help`.*version-pinned public source/);
  assert.match(cli, /not proof of root cause/);
  assert.match(cli, /No PATH changes, installs, updates, reloads, restarts or host\/home switches/);
});

test('fallback prose protects authority and holds unsupported mutation equivalence', () => {
  const fallback = section('Select transport per operation');
  assert.match(fallback, /intentional provider restrictions.*explicit human MCP-only restriction, even for reads/);
  assert.match(fallback, /Read-only daemon\/status\/known-agent inspection.*Verified CLI/);
  assert.match(fallback, /no CLI follow-up\/completion handling/);
  assert.match(fallback, /Unsupported equivalence holds that operation/);
  assert.match(fallback, /ownership\/reservations and current lease\/gate/);
  assert.match(fallback, /synchronous versus queued execution, receiver acceptance\/completion semantics/);
  assert.match(fallback, /Record intent before issuing it.*actual external result afterward/);
  assert.match(fallback, /retry an uncertain operation through a different transport/);
});

test('heartbeat prose holds lossy creation and distinguishes exact-owner delete receipts', () => {
  const heartbeat = section('Heartbeat evidence at Paseo 0.8.0');
  assert.match(heartbeat, /Creation stays held when MCP is unavailable/);
  assert.match(heartbeat, /seven characters.*omits prompt, full target ID, `expiresAt` and `maxRuns`/);
  assert.match(heartbeat, /Do not fill the gaps from the request/);
  assert.match(heartbeat, /First close the affected dispatch gate/);
  assert.match(heartbeat, /Only that target role executes its own deletion/);
  assert.match(heartbeat, /MCP `delete_heartbeat` returns `\{success: true\}`/);
  assert.match(heartbeat, /\{id, status: "deleted"\}.*returned full `id`.*exact requested owned ID/);
  assert.match(heartbeat, /not-found, timeout, transport error.*not acknowledged deletion/);
  assert.match(heartbeat, /Never use public schedule inspect\/list \(MCP or CLI\)/);
  assert.match(heartbeat, /exact old-job absence.*released\/fenced old lease.*intent-before-create/);
  assert.match(heartbeat, /absolute expiry and remaining run budget/);
  assert.match(heartbeat, /uncertain create waits for reconciliation before any retry/);
  assert.match(heartbeat, /helper-only board writes, fresh pass leases/);
});

test('source citations stay version-pinned and paused dispatch gains no new route', () => {
  const links = [...guide.matchAll(/https:\/\/github\.com\/getpaseo\/paseo\/(?:blob|tree)\/([^/)]+)/g)];
  assert.ok(links.length >= 8);
  assert.deepEqual([...new Set(links.map(link => link[1]))],
    ['b8e24677e12b226c7c38c1c3a40649daa9f1152f']);
  assert.match(normalized(guide), /No timerless diagnostic dispatch path is introduced/);
  assert.match(normalized(read('RUN.md')), /not a new timerless route/);
  assert.match(normalized(read('STATE.md')), /passing local validation cannot certify an external operation/);
});
