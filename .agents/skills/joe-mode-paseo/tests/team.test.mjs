import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { summarize, transact } from '../scripts/state.mjs';

const permissions = { provider: 'copilot', modeId: 'agent', features: { auto_accept: true } };
const proof = { parent: permissions, child: permissions, authority: 'human/current-grant', evidence: 'runtime/readback' };
const config = {
  id: 'team', repository: 'forge/org/repo', commonDir: '/repo/.git', cwd: '/repo/main',
  projectId: 'project', workspaceId: 'main', humanOrigin: 'human/kickoff', anchor: 'goal',
  setupEvidence: 'setup', authority: 'human/team', capabilities: 'runtime', mapping: 'mapping',
  retirement: 'preserve-and-retire', merge: 'human', team: true,
  wakeupMode: 'heartbeat', pmAgentId: 'pm', wakeupConsent: 'human/team', cron: '*/5 * * * *',
};
const job = { id: 'pm-job', kind: 'heartbeat', targetAgentId: 'pm', enabled: true,
  cwd: config.cwd, projectId: config.projectId, workspaceId: config.workspaceId,
  cron: config.cron, evidence: 'create-receipt', observation: 'initial-pass', settings: 'approved-settings' };

function board(t, selected = config) {
  const dir = mkdtempSync(path.join(tmpdir(), 'joe-team-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const file = path.join(dir, 'board.json');
  transact(file, { op: 'init', config: selected });
  transact(file, { op: 'resume', human: 'human/start', schedule: job });
  const lease = transact(file, { op: 'claim', owner: 'pm', reconciliation: 'live-owners' }).state.pm.lease;
  const call = request => transact(file, { ...lease, ...request });
  const reserve = (key, work = 'bug', kind = 'delivery', coverage = [key]) =>
    call({ op: 'reserve', worker: { key, kind, coverage, packet: `packets/${key}`,
      ...(kind === 'delivery' ? { work } : {}) } });
  const bind = (key, extra = {}) => call({ op: 'bind', key, agentId: `agent-${key}`,
    worktree: `/worktrees/${key}`, permissions: proof, evidence: 'first-observation', ...extra });
  return { file, call, reserve, bind };
}

function settle(b, key) {
  b.call({ op: 'settle', key, evidence: 'no-live-writers', result: 'preserved-result',
    acceptance: 'receiver/readback', noLiveWriters: true, noUntransferredDuties: true, discoveryEnded: true });
  b.call({ op: 'archive', key, evidence: 'archive/readback' });
}

test('developer pool fits three features, two features plus two fixes, or six fixes', t => {
  for (const mix of [
    ['feature', 'feature', 'feature'],
    ['feature', 'feature', 'bug', 'refactor'],
    ['bug', 'bug', 'hardening', 'refactor', 'bug', 'bug'],
  ]) {
    const b = board(t);
    mix.forEach((work, i) => b.reserve(`issue-${i}`, work));
    assert.throws(() => b.reserve('overflow'), /capacity/);
    assert.equal(b.reserve('support', undefined, 'research').status, 'reserved');
    assert.equal(b.reserve('review', undefined, 'roast').status, 'reserved');
    assert.equal(b.reserve('duck', undefined, 'investigator').status, 'reserved');
    assert.throws(() => b.reserve('overlap', 'bug', 'delivery', ['issue-0']), /coverage/);
  }
});

test('new team deliveries require a known work kind and retain pending reservations', t => {
  const b = board(t);
  assert.throws(() => b.reserve('bad', 'unknown'), /work kind/);
  assert.throws(() => b.call({ op: 'reserve', worker: {
    key: 'missing', kind: 'delivery', packet: 'packet', coverage: ['missing'],
  } }), /work kind/);
  b.reserve('feature', 'feature');
  assert.equal(b.reserve('feature', 'feature').status, 'reused');
  assert.throws(() => b.reserve('feature', 'bug'), /assignment/);
  b.reserve('feature2', 'feature');
  b.reserve('feature3', 'feature');
  b.call({ op: 'release', result: 'pass', duties: 'pending-launches' });
  const lease = transact(b.file, { op: 'claim', owner: 'pm', reconciliation: 'live' }).state.pm.lease;
  assert.throws(() => transact(b.file, { ...lease, op: 'reserve', worker: {
    key: 'next', kind: 'delivery', work: 'bug', packet: 'next', coverage: ['next'],
  } }), /capacity/);
});

test('child binding compares provider, mode and permission features, not labels alone', t => {
  const b = board(t);
  b.reserve('one');
  for (const child of [
    { ...permissions, features: { auto_accept: false } },
    { ...permissions, modeId: 'allow-all' },
    { ...permissions, provider: 'other-provider' },
  ]) {
    assert.throws(() => b.bind('one', { permissions: { ...proof, child } }), /permission/i);
  }
  assert.throws(() => b.bind('one', { permissions: undefined }), /permission/i);
  assert.equal(b.bind('one').status, 'bound');
  assert.deepEqual(JSON.parse(readFileSync(b.file)).pm.workers[0].permissions, proof);
});

test('cross-provider binding needs recorded prelaunch mapping and unchanged native snapshots', t => {
  const b = board(t);
  b.reserve('mixed');
  const target = { provider: 'target-provider', modeId: 'review', features: { approval: 'ask' } };
  const mapped = { ...proof, child: target, preflight: 'launch-mixed',
    parentAgentId: 'pm', workspaceId: 'mixed-workspace' };
  const bind = permissions => b.bind('mixed', { permissions });
  assert.throws(() => bind(mapped), /permission|preflight/i);
  const preflight = { op: 'permission-preflight', key: 'mixed', launchId: 'launch-mixed',
    parentAgentId: 'pm', workspaceId: 'mixed-workspace', worktree: '/worktrees/mixed',
    parent: permissions, target,
    authority: proof.authority, evidence: 'live-parent-and-target-profile',
    mapping: { kind: 'equivalent', preservesChoices: true,
      sourceCapabilities: 'source-native-docs-and-readback',
      targetCapabilities: 'target-native-docs-and-profile',
      rationale: 'Both request approval for the same operations; no approval or credentials transfer',
      evidence: 'verified-policy-comparison' } };
  assert.equal(b.call(preflight).status, 'permission-preflight-recorded');
  assert.throws(() => b.call({ ...preflight, launchId: 'bad', mapping: undefined }), /mapping/i);
  assert.throws(() => b.call({ ...preflight, launchId: 'bad', worktree: undefined }), /worktree/i);
  assert.throws(() => b.call({ ...preflight, launchId: 'bad', mapping: { ...preflight.mapping, preservesChoices: false } }), /permission|choices/i);
  assert.throws(() => b.call({ ...preflight, target: { ...target, modeId: 'unrestricted' } }), /Changed/);
  assert.throws(() => bind(mapped), /launch receipt/i);
  const launch = { op: 'permission-launch', key: 'mixed', launchId: 'launch-mixed',
    agentId: 'agent-mixed', workspaceId: 'mixed-workspace', worktree: '/worktrees/mixed',
    evidence: 'create-agent-receipt-and-child-readback' };
  assert.throws(() => b.call({ ...launch, worktree: '/elsewhere' }), /placement/i);
  assert.equal(b.call(launch).status, 'permission-launch-recorded');
  for (const changed of [
    { parent: { ...permissions, features: { auto_accept: false } } },
    { child: { ...target, modeId: 'unrestricted' } },
    { parentAgentId: 'another-parent' }, { workspaceId: 'elsewhere' },
    { authority: 'different-grant' },
  ]) assert.throws(() => bind({ ...mapped, ...changed }), /permission|preflight/i);
  assert.throws(() => b.bind('mixed', { permissions: mapped, agentId: 'other-child' }), /launch/i);
  assert.equal(bind(mapped).status, 'bound');
  assert.deepEqual(JSON.parse(readFileSync(b.file)).pm.workers[0].permissions.child, target);
  assert.throws(() => b.call({ ...preflight, launchId: 'after-launch' }), /already bound/i);
});

test('feature staffing is two real developers; duplicate and excess developer bindings fail', t => {
  const b = board(t);
  b.reserve('feature', 'feature');
  b.bind('feature');
  const staff = (agentId, worktree) => b.call({ op: 'staff', key: 'feature', agentId, worktree,
    permissions: proof, evidence: 'worker/readback' });
  staff('red', '/red');
  assert.throws(() => staff('green', '/red'), /worktree/);
  staff('green', '/green');
  assert.throws(() => staff('third', '/third'), /capacity/);
  assert.equal(staff('red', '/red').status, 'staffed');
  b.reserve('bug');
  b.bind('bug');
  assert.throws(() => b.call({ op: 'staff', key: 'bug', agentId: 'red', worktree: '/red',
    permissions: proof, evidence: 'readback' }), /developer|worktree/);
});

test('shared persistent roles are singleton reservations outside developer capacity', t => {
  const b = board(t);
  for (const kind of ['shepherd', 'discovery', 'coordinator']) {
    if (kind === 'coordinator') {
      assert.throws(() => b.reserve(kind, undefined, kind), /merge|coordinator/i);
      continue;
    }
    b.reserve(kind, undefined, kind);
    assert.throws(() => b.reserve(`${kind}-duplicate`, undefined, kind), /reserved/);
  }
  for (let i = 0; i < 6; i++) b.reserve(`bug-${i}`);
});

test('heartbeat creation is target-bound, pending cleanup fences retirement, pause prevents new timers', t => {
  const b = board(t);
  b.reserve('shepherd', undefined, 'shepherd');
  b.bind('shepherd');
  const beat = (action, extra = {}) => b.call({ op: 'role-heartbeat', key: 'shepherd', action,
    evidence: 'runtime/receipt', ...extra });
  beat('plan', { settings: 'approved-settings' });
  assert.throws(() => settle(b, 'shepherd'), /heartbeat/);
  assert.throws(() => beat('created', { id: 'timer', targetAgentId: 'pm' }), /target/);
  beat('created', { id: 'timer', targetAgentId: 'agent-shepherd' });
  assert.throws(() => beat('plan', { settings: 'other' }), /heartbeat/);
  transact(b.file, { op: 'pause', human: 'human/pause', disposition: 'preserve-workers' });
  beat('uncertain', { id: 'timer' });
  assert.throws(() => beat('deleted', { id: 'wrong' }), /heartbeat/);
  beat('deleted', { id: 'timer' });
  assert.throws(() => beat('plan', { settings: 'approved-settings' }), /enabled/);
  settle(b, 'shepherd');
  assert.equal(JSON.parse(readFileSync(b.file)).pm.workers[0].heartbeat.status, 'deleted');
});

test('fresh-context retry happens once per issue; repeated blocker is withheld from delivery', t => {
  const b = board(t);
  const block = (key, extra = {}) => b.call({ op: 'block', key, issue: 'forge/repo#1',
    investigator: `duck-${key}`, selfReview: 'developer/attempts', challenge: 'independent/findings',
    missing: 'required answer', category: 'work', evidence: 'blocked/receipt', ...extra });
  b.reserve('first', 'bug', 'delivery', ['forge/repo#1']);
  b.bind('first');
  assert.throws(() => block('first', { investigator: 'agent-first' }), /independent/);
  assert.equal(block('first').status, 'retry');
  assert.equal(block('first').state.pm.blockers[0].attempts.length, 1);
  settle(b, 'first');
  b.reserve('second', 'bug', 'delivery', ['forge/repo#1']);
  assert.throws(() => b.bind('second', { worktree: '/worktrees/first' }), /fresh/);
  assert.throws(() => b.bind('second', { agentId: 'agent-first' }), /fresh/);
  b.bind('second');
  assert.equal(block('second').status, 'blocked');
  settle(b, 'second');
  assert.throws(() => b.reserve('third', 'bug', 'delivery', ['forge/repo#1']), /blocked/);
  b.call({ op: 'unblock', issue: 'forge/repo#1', resolution: 'discovery/answer', readiness: 'tracker/ready' });
  assert.equal(b.reserve('third', 'bug', 'delivery', ['forge/repo#1']).status, 'reserved');
});

test('permission and human-decision blockers cannot trigger fresh-agent retries', t => {
  for (const category of ['permission', 'human']) {
    const b = board(t);
    b.reserve('first');
    b.bind('first');
    assert.equal(b.call({ op: 'block', key: 'first', issue: 'first', investigator: 'duck',
      selfReview: 'attempts', challenge: 'independent', missing: 'human grant',
      category, evidence: 'receipt' }).status, 'blocked');
    settle(b, 'first');
    assert.throws(() => b.reserve('retry', 'bug', 'delivery', ['first']), /blocked/);
    assert.throws(() => b.call({ op: 'unblock', issue: 'first',
      resolution: 'answer', readiness: 'ready' }), /human/);
  }
});

test('retry excludes every prior developer context and worktree, including retired members', t => {
  const b = board(t);
  b.reserve('first', 'feature', 'delivery', ['issue']);
  b.bind('first');
  const staff = (key, agentId, worktree) => b.call({ op: 'staff', key, agentId, worktree,
    permissions: proof, evidence: 'developer/readback' });
  staff('first', 'retired-red', '/actual/red');
  b.call({ op: 'retire-developer', key: 'first', agentId: 'retired-red',
    noLiveWriters: true, noUntransferredDuties: true, result: 'saved',
    acceptance: 'accepted', archive: 'archived', evidence: 'retired' });
  staff('first', 'green', '/actual/green');
  b.call({ op: 'block', key: 'first', issue: 'issue', investigator: 'duck',
    selfReview: 'attempts', challenge: 'independent', missing: 'answer',
    category: 'work', evidence: 'blocked' });
  settle(b, 'first');
  b.reserve('retry', 'feature', 'delivery', ['issue']);
  for (const [agentId, worktree] of [
    ['retired-red', '/fresh'], ['fresh', '/actual/red'],
    ['green', '/fresh'], ['fresh', '/actual/green'],
  ]) {
    assert.throws(() => b.bind('retry', { agentId, worktree }), /fresh/);
  }
  b.bind('retry');
  for (const [agentId, worktree] of [
    ['retired-red', '/fresh'], ['fresh', '/actual/red'],
    ['green', '/fresh'], ['fresh', '/actual/green'],
    ['agent-first', '/fresh'], ['fresh', '/worktrees/first'],
  ]) assert.throws(() => staff('retry', agentId, worktree), /fresh/);
  staff('retry', 'new-red', '/fresh/red');
  const board_ = JSON.parse(readFileSync(b.file)).pm;
  const attempt = board_.blockers[0].attempts[0];
  assert.equal(attempt.key, 'first');
  const blocked = board_.workers.find(worker => worker.key === attempt.key);
  assert.equal(blocked.worktree, '/worktrees/first');
  assert.deepEqual(blocked.developers.map(({ agentId, worktree }) => ({ agentId, worktree })), [
    { agentId: 'retired-red', worktree: '/actual/red' },
    { agentId: 'green', worktree: '/actual/green' },
  ]);
});

test('local cleanup needs archived custody, clean files and matching verified remote head', t => {
  const b = board(t);
  b.reserve('first');
  b.bind('first');
  const preserve = { op: 'cleanup-ready', key: 'first', noLiveWriters: true, clean: true,
    branch: 'refs/heads/recovery/first', localHead: 'a'.repeat(40), remoteHead: 'a'.repeat(40),
    evidence: 'git/remote-readback' };
  assert.throws(() => b.call(preserve), /settled|archive/);
  settle(b, 'first');
  assert.throws(() => b.call({ ...preserve, remoteHead: 'b'.repeat(40) }), /remote/);
  assert.throws(() => b.call({ ...preserve, clean: false }), /unpreserved/);
  assert.equal(b.call(preserve).status, 'cleanup-ready');
  assert.equal(b.call({ op: 'cleanup', key: 'first', evidence: 'worktree/removal-readback' }).status, 'cleaned');
});

test('legacy capacity is never silently reinterpreted; team upgrade requires a paused empty pool', t => {
  const { team, ...legacy } = config;
  const b = board(t, legacy);
  b.call({ op: 'reserve', worker: { key: 'old', kind: 'delivery', packet: 'old', coverage: ['old'] } });
  const upgrade = { op: 'enable-team', human: 'human/team', reconciliation: 'all-owners-and-jobs' };
  assert.throws(() => transact(b.file, upgrade), /paused/);
  transact(b.file, { op: 'pause', human: 'pause', disposition: 'preserve' });
  assert.throws(() => transact(b.file, upgrade), /lease/);
  b.call({ op: 'release', result: 'first-pass', duties: 'old-worker' });
  assert.throws(() => transact(b.file, upgrade), /Settle/);
  transact(b.file, { op: 'resume', human: 'human/reconcile', schedule: { ...job, id: 'replacement' },
    replacement: { oldId: job.id, human: 'human/recreate', absence: 'deleted-receipt', reconciliation: 'same-owner' } });
  const next = transact(b.file, { op: 'claim', owner: 'pm', reconciliation: 'live' }).state.pm.lease;
  b.call = request => transact(b.file, { ...next, ...request });
  settle(b, 'old');
  transact(b.file, { op: 'pause', human: 'pause', disposition: 'settled' });
  b.call({ op: 'release', result: 'pass', duties: 'paused' });
  assert.equal(transact(b.file, upgrade).state.pm.config.team, true);
  assert.equal(transact(b.file, { op: 'inspect' }).state.pm.workers[0].assignment.work, undefined);
});

test('paused legacy migration reaches the first team assignment without reopening dispatch for cleanup', t => {
  const { team, ...legacy } = config;
  const b = board(t, legacy);
  b.call({ op: 'reserve', worker: { key: 'old-backlog', kind: 'discovery',
    packet: 'unaligned-findings', coverage: ['needs-human'] } });
  b.call({ op: 'bind', key: 'old-backlog', agentId: 'retained-backlog', evidence: 'startup' });
  b.call({ op: 'record', key: 'old-return', status: 'pending', evidence: 'awaiting-custody' });
  transact(b.file, { op: 'pause', human: 'human/team', disposition: 'retain-backlog-and-questions' });
  b.call({ op: 'release', result: 'paused-pass', duties: 'transfer-backlog' });
  const manage = request => transact(b.file, { human: 'human/team',
    reconciliation: 'actual-backlog-ack-and-no-live-writers', ...request });

  assert.equal(manage({ op: 'record', key: 'old-return', status: 'accepted',
    evidence: 'preserved-questions', receiver: 'pm-custody-ack' }).status, 'recorded');
  assert.equal(manage({ op: 'settle', key: 'old-backlog', noLiveWriters: true,
    noUntransferredDuties: true, discoveryEnded: true, evidence: 'administrative-end-ack',
    result: 'unaligned-findings', acceptance: 'pm-temporary-custody' }).status, 'settled');
  const reconciled = transact(b.file, { op: 'inspect' }).state.pm;
  assert.equal(reconciled.mode, 'paused');
  assert.equal(reconciled.lease, null);
  assert.deepEqual(reconciled.schedule, job);
  assert.equal(reconciled.wakeupHistory, undefined);
  assert.equal(reconciled.workers[0].archive, undefined);
  assert.equal(reconciled.pending[0].history.length, 1);

  const upgraded = manage({ op: 'enable-team' }).state.pm;
  assert.equal(upgraded.mode, 'paused');
  assert.equal(upgraded.config.team, true);
  assert.deepEqual(upgraded.workers, reconciled.workers);
  assert.deepEqual(upgraded.pending, reconciled.pending);
  assert.deepEqual(upgraded.runs, reconciled.runs);
  assert.deepEqual(upgraded.config, { ...config, capacity: 6 });
  assert.throws(() => manage({ op: 'resume', schedule: job }), /replacement/);
  manage({ op: 'resume', schedule: { ...job, id: 'team-job' },
    replacement: { oldId: job.id, human: 'human/team',
      absence: 'exact-old-id-deletion-receipt', reconciliation: 'same-pm-and-settings' } });
  const lease = transact(b.file, { op: 'claim', owner: 'pm', reconciliation: 'live-owners' }).state.pm.lease;
  const call = request => transact(b.file, { ...lease, ...request });
  call({ op: 'reserve', worker: { key: 'backlog', kind: 'discovery',
    packet: 'same-unaligned-findings-and-questions', coverage: ['needs-human'] } });
  call({ op: 'bind', key: 'backlog', agentId: 'retained-backlog',
    permissions: proof, evidence: 'accepted-reassignment' });
  call({ op: 'reserve', worker: { key: 'independent', kind: 'delivery', work: 'bug',
    packet: 'ready-independent-issue', coverage: ['independent'] } });
  call({ op: 'bind', key: 'independent', agentId: 'developer', worktree: '/trees/independent',
    permissions: proof, evidence: 'accepted-first-assignment' });
  call({ op: 'release', result: 'initial-pass-with-accepted-workers', duties: 'await-results' });
  const current = transact(b.file, { op: 'inspect' }).state.pm;
  assert.equal(current.mode, 'enabled');
  assert.equal(current.lease, null);
  assert.equal(current.wakeupHistory.length, 1);
  assert.deepEqual(current.workers.filter(worker => !worker.settled).map(worker => worker.agentId),
    ['retained-backlog', 'developer']);
  assert.equal(current.workers[0].return.result, 'unaligned-findings');
});

test('legacy human cleanup preserves authority, lease and settlement gates while paused or stopped', t => {
  for (const mode of ['pause', 'stop']) {
    const { team, ...legacy } = config;
    const b = board(t, legacy);
    b.call({ op: 'reserve', worker: { key: 'old', kind: 'discovery', packet: 'old', coverage: ['old'] } });
    transact(b.file, { op: mode, human: 'human/manage', disposition: 'preserve' });
    const grant = { human: 'human/manage', reconciliation: 'observed-current-custody' };
    const record = { op: 'record', key: 'receipt', status: 'observed', evidence: 'result' };
    assert.throws(() => transact(b.file, { ...grant, ...record }), /lease/);
    b.call({ op: 'release', result: 'pass', duties: 'human-cleanup' });
    const snapshot = readFileSync(b.file, 'utf8');
    for (const missing of [{ human: undefined }, { human: '' }, { reconciliation: undefined }]) {
      assert.throws(() => transact(b.file, { ...grant, ...record, ...missing }), /lease|human|reconciliation/);
      assert.equal(readFileSync(b.file, 'utf8'), snapshot);
    }
    for (const op of ['reserve', 'bind', 'cover', 'permission-preflight', 'role-heartbeat']) {
      assert.throws(() => transact(b.file, { ...grant, op }), /lease/);
      assert.equal(readFileSync(b.file, 'utf8'), snapshot);
    }
    assert.throws(() => b.call(record), /lease/);
    assert.equal(transact(b.file, { op: 'claim', owner: 'pm', reconciliation: 'live' }).status,
      mode === 'pause' ? 'paused' : 'stopped');
    const settlement = { ...grant, op: 'settle', key: 'old', evidence: 'actual-end',
      result: 'questions-preserved', acceptance: 'receiver', noLiveWriters: true,
      noUntransferredDuties: true, discoveryEnded: true };
    for (const missing of [{ noLiveWriters: false }, { noUntransferredDuties: false },
      { discoveryEnded: false }, { acceptance: '' }]) {
      assert.throws(() => transact(b.file, { ...settlement, ...missing }), /custody|Discovery|acceptance/);
      assert.equal(readFileSync(b.file, 'utf8'), snapshot);
    }
    assert.throws(() => transact(b.file, { ...grant, op: 'archive', key: 'old', evidence: 'archive' }), /settled/);
    assert.equal(transact(b.file, settlement).status, 'settled');
    assert.equal(transact(b.file, { ...grant, op: 'archive', key: 'old', evidence: 'actual-archive' }).status,
      'archive-recorded');
    const current = transact(b.file, { op: 'inspect' }).state.pm;
    assert.equal(current.mode, mode === 'pause' ? 'paused' : 'stopped');
    assert.equal(current.config.team, undefined);
    assert.equal(current.lease, null);
  }
});

test('corrupt blocker history fails closed instead of resetting the retry budget', t => {
  const b = board(t);
  b.reserve('first');
  b.bind('first');
  b.call({ op: 'block', key: 'first', issue: 'first', investigator: 'duck', selfReview: 'attempts',
    challenge: 'independent', missing: 'answer', category: 'permission', evidence: 'receipt' });
  const original = JSON.parse(readFileSync(b.file));
  for (const corrupt of [
    state => { state.pm.blockers[0].status = 'retry'; },
    state => { state.pm.blockers[0].attempts = []; },
    state => { state.pm.blockers.push(state.pm.blockers[0]); },
  ]) {
    const state = structuredClone(original);
    corrupt(state);
    writeFileSync(b.file, JSON.stringify(state));
    assert.throws(() => transact(b.file, { op: 'inspect' }), /blocker/);
  }
});

test('unknown creation can accept its recovered receipt during paused human cleanup', t => {
  const b = board(t);
  b.reserve('backlog', undefined, 'discovery');
  b.bind('backlog');
  b.call({ op: 'role-heartbeat', key: 'backlog', action: 'plan', settings: 'approved', evidence: 'intent' });
  b.call({ op: 'role-heartbeat', key: 'backlog', action: 'uncertain', evidence: 'transport-failed' });
  transact(b.file, { op: 'pause', human: 'pause', disposition: 'retain-for-cleanup' });
  b.call({ op: 'release', result: 'paused', duties: 'unknown-heartbeat' });
  const manage = { op: 'role-heartbeat', key: 'backlog', human: 'human/pause',
    reconciliation: 'all-current-owners', evidence: 'recovered-receipt' };
  assert.equal(transact(b.file, { ...manage, action: 'created', id: 'found',
    targetAgentId: 'agent-backlog' }).status, 'heartbeat-recorded');
  assert.equal(transact(b.file, { ...manage, action: 'deleted', id: 'found',
    evidence: 'exact-delete-success' }).status, 'heartbeat-recorded');
});

test('a feature lane can retire individual developers and run its next task within the same two slots', t => {
  const b = board(t);
  b.reserve('feature', 'feature');
  b.bind('feature');
  const staff = agentId => b.call({ op: 'staff', key: 'feature', agentId, worktree: `/trees/${agentId}`,
    permissions: proof, evidence: 'first-observation' });
  staff('red');
  staff('green');
  const retire = { op: 'retire-developer', key: 'feature', agentId: 'red', evidence: 'stopped-readback',
    result: 'saved-commit', acceptance: 'receiver/readback', archive: 'actual-archive-readback',
    noLiveWriters: true, noUntransferredDuties: true };
  assert.throws(() => b.call({ ...retire, archive: '' }), /archive/);
  assert.throws(() => b.call({ ...retire, noLiveWriters: false }), /custody/);
  assert.throws(() => staff('next'), /capacity/);
  assert.equal(b.call(retire).status, 'developer-retired');
  assert.equal(staff('next').status, 'staffed');
  assert.throws(() => staff('overflow'), /capacity/);
  const state = JSON.parse(readFileSync(b.file)).pm;
  assert.equal(state.workers[0].settled, false);
  assert.equal(state.workers[0].developers.length, 3);
  assert.equal(state.workers[0].developers[0].return.archive, 'actual-archive-readback');
});

test('human management accepts late returns and retirement after pause/stop without dispatch authority', t => {
  for (const [control, finish] of [['pause', 'release'], ['stop', 'release'], ['stop', 'recover']]) {
    const b = board(t);
    b.reserve('shepherd', undefined, 'shepherd');
    b.bind('shepherd');
    b.call({ op: 'role-heartbeat', key: 'shepherd', action: 'plan', settings: 'settings', evidence: 'intent' });
    b.call({ op: 'role-heartbeat', key: 'shepherd', action: 'created', id: 'job',
      targetAgentId: 'agent-shepherd', evidence: 'receipt' });
    transact(b.file, { op: control, human: 'human/stop', disposition: 'retain-for-return' });
    if (finish === 'release') b.call({ op: 'release', result: 'stopped', duties: 'late-role-return' });
    else transact(b.file, { op: 'recover', human: 'human/recovery',
      token: JSON.parse(readFileSync(b.file)).pm.lease.token,
      fencing: 'old-pass-stopped', reconciliation: 'late-role-return' });
    const management = { human: 'human/stop', reconciliation: 'stopped-owners-and-pending-jobs' };
    const manage = request => transact(b.file, { ...management, ...request });
    manage({ op: 'role-heartbeat', key: 'shepherd', action: 'deleted', id: 'job', evidence: 'delete-success' });
    manage({ op: 'record', key: 'late-result', status: 'accepted', evidence: 'result', receiver: 'PM/readback' });
    assert.equal(manage({ op: 'settle', key: 'shepherd', result: 'result', acceptance: 'readback',
      noLiveWriters: true, noUntransferredDuties: true, evidence: 'terminal' }).status, 'settled');
    assert.equal(manage({ op: 'archive', key: 'shepherd', evidence: 'archive/readback' }).status, 'archive-recorded');
    assert.throws(() => manage({ op: 'reserve', worker: {
      key: 'new', kind: 'research', coverage: ['new'], packet: 'new',
    } }), /lease/);
    assert.throws(() => b.call({ op: 'record', key: 'stale', status: 'observed', evidence: 'stale' }), /lease/);
    assert.equal(transact(b.file, { op: 'claim', owner: 'pm', reconciliation: 'live' }).status,
      control === 'pause' ? 'paused' : 'stopped');
  }
});

test('definitive heartbeat absence permits retirement or replanning, unknown creation does not', t => {
  for (const [uncertain, outcome] of [[false, 'retire'], [true, 'replan']]) {
    const b = board(t);
    b.reserve('shepherd', undefined, 'shepherd');
    b.bind('shepherd');
    const beat = (action, extra = {}) => b.call({ op: 'role-heartbeat', key: 'shepherd', action,
      evidence: 'runtime', ...extra });
    beat('plan', { settings: 'approved' });
    if (uncertain) beat('uncertain');
    assert.throws(() => beat('absent'), /absence/);
    assert.throws(() => beat('plan', { settings: 'approved' }), /heartbeat/);
    assert.equal(beat('absent', { absence: 'verified-no-external-effect' }).status, 'heartbeat-recorded');
    if (outcome === 'retire') settle(b, 'shepherd');
    else assert.equal(beat('plan', { settings: 'approved' }).status, 'heartbeat-recorded');
  }
});

test('the bounded team view shows live lanes and open blockers without permission or return detail', t => {
  const b = board(t);
  b.reserve('lane', 'feature');
  b.bind('lane');
  for (const agentId of ['red', 'green']) {
    b.call({ op: 'staff', key: 'lane', agentId, worktree: `/worktrees/${agentId}`,
      permissions: proof, evidence: 'developer/readback' });
  }
  b.call({ op: 'retire-developer', key: 'lane', agentId: 'red', noLiveWriters: true,
    noUntransferredDuties: true, result: 'preserved', acceptance: 'accepted',
    archive: 'archived/readback', evidence: 'retired/readback' });
  b.call({ op: 'block', key: 'lane', issue: 'lane', investigator: 'duck', selfReview: 'attempts',
    challenge: 'independent', missing: 'answer', category: 'work', evidence: 'blocker/evidence' });
  const view = summarize(JSON.parse(readFileSync(b.file)));
  assert.equal(view.team, true);
  assert.deepEqual(view.workers, [{ key: 'lane', kind: 'delivery', coverage: ['lane'], work: 'feature',
    agentId: 'agent-lane', worktree: '/worktrees/lane',
    developers: [{ agentId: 'green', worktree: '/worktrees/green' }], retiredDevelopers: 1 }]);
  assert.deepEqual(view.blockers, [{ issue: 'lane', status: 'retry', attempts: 1 }]);
  const text = JSON.stringify(view);
  for (const omitted of ['runtime/readback', 'archived/readback', 'blocker/evidence', 'packets/lane']) {
    assert.ok(!text.includes(omitted), `bounded team view leaked ${omitted}`);
  }
  b.call({ op: 'unblock', issue: 'lane', resolution: 'answer/delivered', readiness: 'verified/ready' });
  const resolved = summarize(JSON.parse(readFileSync(b.file)));
  assert.equal(resolved.blockers, undefined);
  assert.equal(resolved.history.blockers, 1);
});

test('a lane staffed after one covered issue blocks keeps working and still bars those contexts on retry', t => {
  const b = board(t);
  b.reserve('lane', 'feature', 'delivery', ['blocked-issue', 'other-issue']);
  b.bind('lane');
  const staff = (key, agentId, worktree) => b.call({ op: 'staff', key, agentId, worktree,
    permissions: proof, evidence: 'developer/readback' });
  staff('lane', 'first', '/actual/first');
  const blocker = { op: 'block', key: 'lane', issue: 'blocked-issue', investigator: 'duck',
    selfReview: 'attempts', challenge: 'independent', missing: 'answer',
    category: 'work', evidence: 'blocked/evidence' };
  b.call(blocker);
  assert.equal(staff('lane', 'later', '/actual/later').status, 'staffed');
  assert.equal(b.call(blocker).status, 'retry');
  assert.equal(b.call({ op: 'inspect' }).status, 'observed');
  settle(b, 'lane');
  b.reserve('retry', 'feature', 'delivery', ['blocked-issue']);
  for (const [agentId, worktree] of [['first', '/fresh'], ['later', '/fresh'],
    ['fresh', '/actual/first'], ['fresh', '/actual/later']]) {
    assert.throws(() => b.bind('retry', { agentId, worktree }), /fresh/);
  }
  b.bind('retry');
});

const target = { provider: 'target-provider', modeId: 'review', features: { approval: 'ask' } };
const mapping = { kind: 'equivalent', preservesChoices: true,
  sourceCapabilities: 'source-native-docs-and-readback',
  targetCapabilities: 'target-native-docs-and-profile',
  rationale: 'Both request approval for the same operations',
  evidence: 'verified-policy-comparison' };

test('a cross-provider preflight authorizes only its planned worktree and actually launched child', t => {
  const b = board(t);
  b.reserve('lane', 'feature');
  b.bind('lane');
  const plan = { op: 'permission-preflight', key: 'lane', launchId: 'launch-one', purpose: 'staff',
    parentAgentId: 'agent-lane', workspaceId: 'workspace-red', worktree: '/planned/red',
    parent: permissions, target, authority: proof.authority,
    evidence: 'live-parent-and-target-profile', mapping };
  assert.equal(b.call(plan).status, 'permission-preflight-recorded');
  const mapped = { ...proof, child: target, preflight: 'launch-one',
    parentAgentId: 'agent-lane', workspaceId: 'workspace-red' };
  const staff = (agentId, worktree) => b.call({ op: 'staff', key: 'lane', agentId, worktree,
    permissions: mapped, evidence: 'developer/readback' });
  assert.throws(() => staff('actual-red', '/planned/red'), /launch/i);
  const receipt = { op: 'permission-launch', key: 'lane', launchId: 'launch-one',
    agentId: 'actual-red', workspaceId: 'workspace-red', worktree: '/planned/red',
    evidence: 'create-agent-receipt-and-child-readback' };
  assert.equal(b.call(receipt).status, 'permission-launch-recorded');
  assert.throws(() => b.call({ ...receipt, agentId: 'other-child' }), /Changed/);
  assert.throws(() => staff('other-child', '/planned/red'), /launch/i);
  assert.throws(() => staff('actual-red', '/elsewhere'), /launch|worktree/i);
  assert.equal(staff('actual-red', '/planned/red').status, 'staffed');
  assert.throws(() => b.call({ op: 'staff', key: 'lane', agentId: 'second-child',
    worktree: '/planned/green', permissions: mapped, evidence: 'developer/readback' }), /launch|preflight/i);
});

test('the bounded view keeps a retirement queue until each settled worker is actually terminal', t => {
  const b = board(t);
  const view = () => summarize(JSON.parse(readFileSync(b.file)));
  const queue = () => view().retirement;
  b.reserve('lane');
  b.bind('lane');
  b.reserve('helper', undefined, 'roast', ['lane']);
  b.bind('helper', { worktree: undefined });
  b.call({ op: 'settle', key: 'lane', evidence: 'no-live-writers', result: 'preserved',
    acceptance: 'receiver/readback', noLiveWriters: true, noUntransferredDuties: true });
  assert.deepEqual(queue(), [{ key: 'lane', kind: 'delivery', agentId: 'agent-lane',
    worktree: '/worktrees/lane', phase: 'archive-pending' }]);
  b.call({ op: 'archive', key: 'lane', evidence: 'archive/readback' });
  assert.deepEqual(queue(), [{ key: 'lane', kind: 'delivery', agentId: 'agent-lane',
    worktree: '/worktrees/lane', phase: 'cleanup-pending' }]);
  const head = 'a'.repeat(40);
  b.call({ op: 'cleanup-ready', key: 'lane', noLiveWriters: true, clean: true,
    branch: 'refs/heads/recovery/lane', localHead: head, remoteHead: head, evidence: 'preserved/remote' });
  assert.deepEqual(queue(), [{ key: 'lane', kind: 'delivery', agentId: 'agent-lane',
    worktree: '/worktrees/lane', phase: 'removal-pending',
    recovery: { branch: 'refs/heads/recovery/lane', head } }]);
  b.call({ op: 'cleanup', key: 'lane', evidence: 'removal/readback' });
  assert.equal(queue(), undefined);
  assert.equal(view().history.settledWorkers, 1);

  b.call({ op: 'settle', key: 'helper', evidence: 'no-live-writers', result: 'review',
    acceptance: 'receiver/readback', noLiveWriters: true, noUntransferredDuties: true });
  assert.deepEqual(queue(), [{ key: 'helper', kind: 'roast', agentId: 'agent-helper',
    phase: 'archive-pending' }]);
  b.call({ op: 'archive', key: 'helper', evidence: 'archive/readback' });
  assert.equal(queue(), undefined, 'a role without an owned worktree is terminal once archived');

  b.reserve('kept');
  b.bind('kept');
  settle(b, 'kept');
  assert.equal(queue()[0].phase, 'cleanup-pending');
  assert.equal(b.call({ op: 'cleanup', key: 'kept', retained: true,
    evidence: 'human-directed/worktree-retained' }).status, 'retained');
  assert.equal(queue(), undefined, 'a deliberately retained worktree is a terminal outcome');
  assert.equal(view().history.settledWorkers, 3);
});

test('an accepted cleanup receipt is immutable and a terminal outcome cannot be erased', t => {
  const b = board(t);
  const worker = () => JSON.parse(readFileSync(b.file)).pm.workers.find(item => item.key === 'lane');
  b.reserve('lane');
  b.bind('lane');
  settle(b, 'lane');
  const head = 'a'.repeat(40);
  const ready = { op: 'cleanup-ready', key: 'lane', noLiveWriters: true, clean: true,
    branch: 'refs/heads/recovery/lane', localHead: head, remoteHead: head, evidence: 'preserved/remote' };
  assert.equal(b.call(ready).status, 'cleanup-ready');
  assert.equal(b.call(ready).status, 'cleanup-ready', 'identical preservation replay is idempotent');
  const other = 'b'.repeat(40);
  assert.throws(() => b.call({ ...ready, branch: 'refs/heads/recovery/other' }), /Changed cleanup preservation/);
  assert.throws(() => b.call({ ...ready, localHead: other, remoteHead: other }), /Changed cleanup preservation/);
  assert.throws(() => b.call({ ...ready, evidence: 'other/remote' }), /Changed cleanup preservation/);
  assert.throws(() => b.call({ ...ready, clean: false }), /Live writers|unpreserved/i);
  assert.deepEqual(worker().cleanup,
    { branch: 'refs/heads/recovery/lane', head, preservation: 'preserved/remote' });
  const removal = { op: 'cleanup', key: 'lane', evidence: 'removal/readback' };
  assert.equal(b.call(removal).status, 'cleaned');
  assert.equal(b.call(removal).status, 'cleaned', 'identical removal replay is idempotent');
  assert.throws(() => b.call({ ...removal, evidence: 'other/readback' }), /Changed cleanup removal/);
  assert.throws(() => b.call(ready), /already removed/i);
  assert.throws(() => b.call({ op: 'cleanup', key: 'lane', retained: true, evidence: 'kept' }), /already removed/i);
  assert.deepEqual(worker().cleanup, { branch: 'refs/heads/recovery/lane', head,
    preservation: 'preserved/remote', removal: 'removal/readback' });
  assert.equal(summarize(JSON.parse(readFileSync(b.file))).retirement, undefined);

  b.reserve('kept');
  b.bind('kept');
  settle(b, 'kept');
  const retain = { op: 'cleanup', key: 'kept', retained: true, evidence: 'human-directed/kept' };
  assert.equal(b.call(retain).status, 'retained');
  assert.equal(b.call(retain).status, 'retained', 'identical retention replay is idempotent');
  assert.throws(() => b.call({ ...retain, evidence: 'other/kept' }), /Changed cleanup retention/);
  assert.throws(() => b.call({ ...ready, key: 'kept' }), /retained/i);
  assert.throws(() => b.call({ op: 'cleanup', key: 'kept', evidence: 'removal/readback' }), /preservation|retained/i);
});

test('a contradictory or malformed cleanup record fails validation', t => {
  const b = board(t);
  b.reserve('lane');
  b.bind('lane');
  settle(b, 'lane');
  const head = 'a'.repeat(40);
  b.call({ op: 'cleanup-ready', key: 'lane', noLiveWriters: true, clean: true,
    branch: 'refs/heads/recovery/lane', localHead: head, remoteHead: head, evidence: 'preserved/remote' });
  const state = JSON.parse(readFileSync(b.file));
  const poison = cleanup => {
    const copy = JSON.parse(JSON.stringify(state));
    copy.pm.workers.find(item => item.key === 'lane').cleanup = cleanup;
    writeFileSync(b.file, JSON.stringify(copy));
    assert.throws(() => b.call({ op: 'inspect' }), /cleanup/i);
  };
  poison({ retention: 'kept', removal: 'removal/readback' });
  poison({ retention: 'kept', branch: 'refs/heads/recovery/lane', head });
  poison({ removal: 'removal/readback' });
  poison({ branch: 'refs/heads/recovery/lane', head, preservation: '' });
  poison('cleaned');
});
