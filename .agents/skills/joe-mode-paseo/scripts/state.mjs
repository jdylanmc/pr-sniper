import { closeSync, existsSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, renameSync, rmdirSync, writeFileSync } from 'node:fs';
import { isDeepStrictEqual } from 'node:util';
import { randomUUID } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { teamKinds, checkTeamReservation, checkTeamBinding, checkTeamState, teamOperation } from './team.mjs';

function requireText(value, label) {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`Missing ${label}`);
}

function validCoverage(coverage) {
  return Array.isArray(coverage) && coverage.length > 0 &&
    coverage.every(id => typeof id === 'string' && id.trim()) &&
    new Set(coverage).size === coverage.length;
}

function validateGraph(worker) {
  if (worker.assignment.graph === undefined) {
    if (worker.graph !== undefined || !isDeepStrictEqual(worker.assignment.coverage, worker.coverage)) {
      throw new Error('Invalid worker coverage');
    }
    return;
  }
  if (worker.assignment.graph !== true || worker.kind !== 'delivery' ||
    !validCoverage(worker.assignment.coverage) || typeof worker.graph?.complete !== 'boolean' ||
    !Array.isArray(worker.graph.receipts)) throw new Error('Invalid publication graph');
  let coverage = worker.assignment.coverage;
  let complete = false;
  for (const receipt of worker.graph.receipts) {
    requireText(receipt.evidence, 'publication evidence');
    if (!validCoverage(receipt.coverage) || typeof receipt.complete !== 'boolean' ||
      coverage.some(id => !receipt.coverage.includes(id)) ||
      (complete && (!receipt.complete || !isDeepStrictEqual(coverage, receipt.coverage)))) {
      throw new Error('Invalid publication receipt');
    }
    coverage = receipt.coverage;
    complete = receipt.complete;
  }
  if (!isDeepStrictEqual(coverage, worker.coverage) || complete !== worker.graph.complete ||
    (worker.agentId && !complete)) throw new Error('Invalid publication state');
}

function publicationPending(pm) {
  return pm.workers.some(worker => !worker.settled && worker.graph && !worker.graph.complete);
}

function validateConfig(input) {
  const config = { ...input, capacity: input?.capacity ?? 6 };
  for (const key of ['id', 'repository', 'commonDir', 'cwd', 'projectId', 'workspaceId',
    'humanOrigin', 'anchor', 'setupEvidence', 'authority', 'capabilities', 'mapping', 'retirement']) {
    requireText(config[key], key);
  }
  if (!['human', 'orchestrator'].includes(config.merge)) throw new Error('Merge mode unsupported');
  if (config.merge === 'orchestrator') {
    for (const key of ['source', 'authority', 'roast', 'ci', 'lint', 'rubberDuck', 'verification']) {
      requireText(config.mergeGate?.[key], `repository merge gate ${key}; clarify with the human`);
    }
  }
  if (!Number.isSafeInteger(config.capacity) || config.capacity < 1) throw new Error('Invalid capacity');
  if (config.cron !== undefined && (typeof config.cron !== 'string' ||
    !/^(?:\*|\*\/(?:[1-9]|[1-5][0-9])) \* \* \* \*$/.test(config.cron))) {
    throw new Error('Invalid PM cadence');
  }
  if (config.wakeupMode !== undefined && !['fresh', 'heartbeat'].includes(config.wakeupMode)) {
    throw new Error('Invalid wakeup mode');
  }
  if (config.wakeupMode === 'heartbeat') {
    requireText(config.wakeupConsent, 'heartbeat consent');
    requireText(config.pmAgentId, 'bound PM agent ID');
  } else if (config.pmAgentId !== undefined) {
    throw new Error('Fresh wakeup cannot bind a heartbeat PM agent');
  }
  if (config.team !== undefined && config.team !== true) throw new Error('Invalid team selection');
  if (config.team && config.wakeupMode !== 'heartbeat') throw new Error('Team mode requires the persistent PM heartbeat');
  if (config.idleShutdown !== undefined) requireText(config.idleShutdown, 'idle shutdown authority');
  return config;
}

function validateJob(config, job) {
  requireText(job?.id, 'wakeup ID');
  requireText(job.evidence, 'wakeup verification evidence');
  requireText(job.observation, 'initial observation');
  const heartbeat = config.wakeupMode === 'heartbeat';
  if (job.enabled !== true || job.cron !== (config.cron ?? '* * * * *') ||
    ['cwd', 'projectId', 'workspaceId'].some(key => job[key] !== config[key]) ||
    (heartbeat ? job.kind !== 'heartbeat' || job.targetAgentId !== config.pmAgentId :
      (job.kind !== undefined && job.kind !== 'schedule') || job.targetAgentId !== undefined)) {
    throw new Error('Unverified schedule binding');
  }
  if (heartbeat) requireText(job.settings, 'approved heartbeat settings');
}

function validateOwner(config, owner) {
  if (config.wakeupMode === 'heartbeat' && owner !== config.pmAgentId) {
    throw new Error('Claim owner must be the bound PM agent');
  }
}

function resume(pm, request) {
  const job = request.schedule;
  validateJob(pm.config, job);
  const heartbeat = pm.config.wakeupMode === 'heartbeat';
  const changed = pm.schedule && pm.schedule.id !== job.id;
  if (changed && !heartbeat) throw new Error('Different schedule; reconcile explicitly');
  if (heartbeat && pm.schedule && (changed || pm.mode !== 'enabled')) {
    if (pm.mode === 'enabled') throw new Error('Heartbeat replacement requires paused or stopped state');
    if (pm.lease) throw new Error('Heartbeat replacement requires released or fenced lease');
    const replacement = request.replacement;
    if (!changed || replacement?.oldId !== pm.schedule.id) throw new Error('Invalid heartbeat replacement');
    requireText(replacement.human, 'replacement human decision');
    requireText(replacement.absence, 'verified old heartbeat absence');
    requireText(replacement.reconciliation, 'replacement reconciliation');
    if (job.settings !== pm.schedule.settings) throw new Error('Changed heartbeat settings');
    pm.wakeupHistory ??= [];
    pm.wakeupHistory.push({ job: pm.schedule, replacement });
  } else if (request.replacement !== undefined) {
    throw new Error('Unexpected heartbeat replacement');
  }
  if (heartbeat && pm.schedule && job.settings !== pm.schedule.settings) throw new Error('Changed heartbeat settings');
  pm.schedule = job;
  pm.mode = 'enabled';
}

function validateState(state) {
  if (!state || typeof state !== 'object' || Array.isArray(state)) throw new Error('Invalid board');
  if (!Object.hasOwn(state, 'pm')) return state;
  const pm = state.pm;
  if (pm?.version !== 1 || !['paused', 'enabled', 'stopped'].includes(pm.mode) ||
    !['workers', 'pending', 'runs'].every(key => Array.isArray(pm[key])) ||
    !isDeepStrictEqual(pm.config, validateConfig(pm.config))) throw new Error('Invalid PM state');
  if (pm.schedule !== undefined) validateJob(pm.config, pm.schedule);
  if (pm.wakeupHistory !== undefined && !Array.isArray(pm.wakeupHistory)) throw new Error('Invalid wakeup history');
  if (pm.mergeHistory !== undefined && !Array.isArray(pm.mergeHistory)) throw new Error('Invalid merge history');
  if (pm.idleShutdownHistory !== undefined && !Array.isArray(pm.idleShutdownHistory)) throw new Error('Invalid idle shutdown history');
  if (pm.mode === 'enabled' && !pm.schedule) throw new Error('Missing schedule binding');
  if (pm.lease !== null) {
    for (const key of ['owner', 'token', 'reconciliation']) requireText(pm.lease?.[key], `Invalid lease ${key}`);
    validateOwner(pm.config, pm.lease.owner);
  }
  const keys = new Set();
  const coverage = new Set();
  let discovery = 0;
  let deliveries = 0;
  for (const worker of pm.workers) {
    if (!worker || typeof worker.settled !== 'boolean' ||
      !(pm.config.team ? teamKinds : ['delivery', 'discovery', 'research']).includes(worker.kind) ||
      typeof worker.key !== 'string' || !worker.key.trim() || keys.has(worker.key) ||
      !validCoverage(worker.coverage) ||
      worker.assignment?.key !== worker.key || worker.assignment?.kind !== worker.kind ||
      !(worker.agentId === null || (typeof worker.agentId === 'string' && worker.agentId.trim()))) {
      throw new Error('Invalid worker state');
    }
    keys.add(worker.key);
    requireText(worker.assignment.packet, 'Invalid worker packet');
    validateGraph(worker);
    if (worker.settled) {
      for (const key of ['evidence', 'result', 'acceptance']) requireText(worker.return?.[key], `Invalid return ${key}`);
      continue;
    }
    if (worker.kind === 'discovery') discovery++;
    if (worker.kind === 'delivery') {
      deliveries++;
      for (const id of worker.coverage) {
        if (coverage.has(id)) throw new Error('Invalid overlapping coverage');
        coverage.add(id);
      }
    }
  }
  if (discovery > 1 || deliveries > pm.config.capacity) throw new Error('Invalid capacity state');
  if (pm.config.team) checkTeamState(pm);
  return state;
}

function reserve(pm, worker) {
  requireText(worker?.key, 'worker key');
  requireText(worker.packet, 'worker packet');
  if (!(pm.config.team ? teamKinds : ['delivery', 'discovery', 'research']).includes(worker.kind)) throw new Error('Invalid worker kind');
  if (!validCoverage(worker.coverage)) throw new Error('Invalid coverage');
  if (worker.graph !== undefined && (worker.graph !== true || worker.kind !== 'delivery')) {
    throw new Error('Invalid publication group');
  }
  const existing = pm.workers.find(item => item.key === worker.key);
  if (existing) {
    if (!isDeepStrictEqual(existing.assignment, worker)) throw new Error('Different worker assignment');
    return 'reused';
  }
  const active = pm.workers.filter(item => !item.settled);
  if (worker.kind === 'discovery' && active.some(item => item.kind === 'discovery')) {
    throw new Error('Discovery conversation already reserved');
  }
  if (worker.kind === 'delivery') {
    if (publicationPending(pm)) throw new Error('Unresolved ticket publication; hold new delivery');
    if (active.some(item => item.kind === 'delivery' && item.coverage.some(id => worker.coverage.includes(id)))) {
      throw new Error('Overlapping delivery coverage');
    }
    if (active.filter(item => item.kind === 'delivery').length >= pm.config.capacity) {
      throw new Error('Delivery capacity exhausted');
    }
  }
  if (pm.config.team) checkTeamReservation(pm, worker);
  pm.workers.push({ key: worker.key, kind: worker.kind, coverage: worker.coverage,
    assignment: worker, settled: false, agentId: null,
    ...(worker.graph ? { graph: { complete: false, receipts: [] } } : {}) });
  return 'reserved';
}

function cover(pm, worker, request) {
  if (worker.settled || !worker.graph) throw new Error('Missing active publication group');
  if (!validCoverage(request.coverage) || typeof request.complete !== 'boolean' ||
    worker.coverage.some(id => !request.coverage.includes(id))) throw new Error('Invalid expanded coverage');
  if (worker.graph.complete && (!request.complete || !isDeepStrictEqual(worker.coverage, request.coverage))) {
    throw new Error('Completed publication coverage is immutable');
  }
  if (pm.workers.some(other => other !== worker && !other.settled && other.kind === 'delivery' &&
    other.coverage.some(id => request.coverage.includes(id)))) throw new Error('Overlapping delivery coverage');
  const receipt = { coverage: request.coverage, complete: request.complete, evidence: request.evidence };
  if (!isDeepStrictEqual(worker.graph.receipts.at(-1), receipt)) worker.graph.receipts.push(receipt);
  worker.coverage = request.coverage;
  worker.graph.complete = request.complete;
  return 'covered';
}

function updateWorker(pm, request) {
  const worker = pm.workers.find(item => item.key === request.key);
  if (!worker) throw new Error('Unknown worker');
  requireText(request.evidence, 'live evidence');
  if (request.op === 'cover') return cover(pm, worker, request);
  if (request.op === 'bind') {
    requireText(request.agentId, 'observed agent ID');
    if (worker.kind === 'delivery' && publicationPending(pm)) throw new Error('Unresolved ticket publication; hold delivery launch');
    if (worker.settled || (worker.agentId && worker.agentId !== request.agentId)) throw new Error('Worker already bound or settled');
    if (pm.config.team) {
      checkTeamBinding(pm, worker, request);
      worker.permissions = request.permissions;
      if (worker.kind === 'delivery') worker.worktree = request.worktree;
    }
    worker.agentId = request.agentId;
    worker.observation = request.evidence;
    return 'bound';
  }
  if (request.op === 'archive') {
    if (!worker.settled) throw new Error('Worker duties not settled');
    worker.archive = request.evidence;
    return 'archive-recorded';
  }
  if (request.noLiveWriters !== true || request.noUntransferredDuties !== true) throw new Error('Unreconciled live custody');
  if (worker.heartbeat && !['deleted', 'absent'].includes(worker.heartbeat.status)) throw new Error('Unresolved owned heartbeat');
  if (worker.kind === 'discovery' && request.discoveryEnded !== true) throw new Error('Discovery alignment or explicit end required');
  requireText(request.result, 'preserved result');
  requireText(request.acceptance, 'receiver acceptance');
  if (worker.settled) throw new Error('Worker already settled');
  worker.settled = true;
  worker.return = { evidence: request.evidence, result: request.result, acceptance: request.acceptance };
  return 'settled';
}

function apply(state, request) {
  if (request.op === 'init') {
    const config = validateConfig(request.config);
    if (state.pm) {
      if (!isDeepStrictEqual(state.pm.config, config)) throw new Error('Existing activation is different');
      return 'existing';
    }
    state.pm = { version: 1, config, mode: 'paused', lease: null, workers: [], pending: [], runs: [] };
    return 'initialized';
  }
  if (request.op === 'inspect') return 'observed';
  const pm = state.pm;
  if (pm?.version !== 1) throw new Error('Missing or unsupported PM state');
  if (['pause', 'stop', 'resume', 'recover', 'configure-merge', 'configure-idle-shutdown', 'enable-team'].includes(request.op)) {
    requireText(request.human, 'human decision');
    if (request.op === 'configure-idle-shutdown') {
      if (pm.mode === 'enabled') throw new Error('Idle shutdown configuration requires paused or stopped state');
      if (pm.lease) throw new Error('Idle shutdown configuration requires released or fenced lease');
      requireText(request.reconciliation, 'idle shutdown authority and child disposition reconciliation');
      requireText(request.idleShutdown, 'idle shutdown authority');
      if (pm.config.idleShutdown !== request.idleShutdown) {
        pm.idleShutdownHistory ??= [];
        pm.idleShutdownHistory.push({ authority: pm.config.idleShutdown ?? null,
          human: request.human, reconciliation: request.reconciliation });
        pm.config = validateConfig({ ...pm.config, idleShutdown: request.idleShutdown });
      }
    } else if (request.op === 'enable-team') {
      if (pm.mode === 'enabled') throw new Error('Team conversion requires paused state');
      if (pm.lease) throw new Error('Team conversion requires released or fenced lease');
      requireText(request.reconciliation, 'all owners and wakeups reconciled');
      if (pm.workers.some(worker => !worker.settled) || pm.pending.some(record => record.status === 'pending')) {
        throw new Error('Settle existing owners and pending operations before changing capacity units');
      }
      pm.config = validateConfig({ ...pm.config, team: true });
    } else if (request.op === 'configure-merge') {
      if (pm.mode === 'enabled') throw new Error('Merge configuration requires paused or stopped state');
      if (pm.lease) throw new Error('Merge configuration requires released or fenced lease');
      requireText(request.reconciliation, 'merge authority and pending-operation reconciliation');
      const { mergeGate, ...previous } = pm.config;
      const config = validateConfig({ ...previous, merge: request.merge,
        ...(request.merge === 'orchestrator' ? { mergeGate: request.mergeGate } : {}) });
      if (!isDeepStrictEqual(pm.config, config)) {
        pm.mergeHistory ??= [];
        pm.mergeHistory.push({ merge: pm.config.merge, ...(mergeGate ? { mergeGate } : {}),
          human: request.human, reconciliation: request.reconciliation });
        pm.config = config;
      }
    } else if (request.op === 'resume') {
      resume(pm, request);
    } else if (request.op === 'recover') {
      requireText(request.fencing, 'stopped-owner fencing evidence');
      requireText(request.reconciliation, 'descendant and partial-work reconciliation');
      if (!pm.lease || pm.lease.token !== request.token) throw new Error('Stale recovery lease');
      pm.runs.push({ ...pm.lease, recovery: request });
      pm.lease = null;
    } else {
      requireText(request.disposition, 'active child disposition');
      pm.mode = request.op === 'pause' ? 'paused' : 'stopped';
    }
    pm.control = request;
    return pm.mode;
  }
  if (request.op === 'claim') {
    validateOwner(pm.config, request.owner);
    if (pm.mode !== 'enabled') return pm.mode;
    if (pm.lease) return 'busy';
    requireText(request.owner, 'run owner');
    requireText(request.reconciliation, 'live repository ownership reconciliation');
    pm.lease = { owner: request.owner, token: randomUUID(), reconciliation: request.reconciliation };
    return 'claimed';
  }
  const management = pm.mode !== 'enabled' && !pm.lease && request.human &&
    (['record', 'settle', 'archive'].includes(request.op) ||
      pm.config.team && ['role-heartbeat', 'retire-developer', 'cleanup-ready', 'cleanup'].includes(request.op));
  if (management) {
    requireText(request.human, 'human management decision');
    requireText(request.reconciliation, 'current custody and pending-operation reconciliation');
  } else if (!pm.lease || pm.lease.owner !== request.owner || pm.lease.token !== request.token) {
    throw new Error('Invalid run lease');
  }
  if (request.op === 'suspend') {
    if (pm.mode !== 'enabled') throw new Error('PM is not enabled');
    requireText(pm.config.idleShutdown, 'idle shutdown authority');
    if (!['waiting-for-human', 'no-useful-work', 'runtime-blocked'].includes(request.reason)) {
      throw new Error('Invalid suspension reason');
    }
    requireText(request.evidence, 'no useful next action evidence');
    requireText(request.disposition, 'active child disposition');
    pm.mode = 'paused';
    pm.control = { op: 'suspend', authority: pm.config.idleShutdown, reason: request.reason,
      evidence: request.evidence, disposition: request.disposition };
    return 'paused';
  }
  if (request.op === 'release') {
    requireText(request.result, 'preserved run result');
    requireText(request.duties, 'remaining duties');
    pm.runs.push({ ...pm.lease, result: request.result, duties: request.duties });
    pm.lease = null;
    return 'released';
  }
  if (request.op === 'reserve' || request.op === 'bind') {
    if (pm.mode !== 'enabled') throw new Error('PM is not enabled');
  }
  if (request.op === 'reserve') return reserve(pm, request.worker);
  if (['permission-preflight', 'permission-launch', 'staff', 'retire-developer', 'role-heartbeat',
    'block', 'unblock', 'cleanup-ready', 'cleanup'].includes(request.op)) {
    return teamOperation(pm, request);
  }
  if (['cover', 'bind', 'settle', 'archive'].includes(request.op)) return updateWorker(pm, request);
  if (request.op === 'record') {
    requireText(request.key, 'operation key');
    requireText(request.evidence, 'operation evidence');
    if (!['pending', 'accepted', 'blocked', 'observed'].includes(request.status)) throw new Error('Invalid record status');
    if (request.status === 'accepted') requireText(request.receiver, 'receiver observation');
    const previous = pm.pending.find(item => item.key === request.key);
    const record = { key: request.key, status: request.status, evidence: request.evidence };
    if (request.status === 'accepted') record.receiver = request.receiver;
    if (previous) {
      const { key, history = [], ...outcome } = previous;
      const { key: ignored, ...nextOutcome } = record;
      if (isDeepStrictEqual(outcome, nextOutcome)) return 'recorded';
      record.history = [...history, outcome];
      pm.pending[pm.pending.indexOf(previous)] = record;
    } else pm.pending.push(record);
    return 'recorded';
  }
  throw new Error('Unsupported operation');
}

function workerView(worker) {
  const active = (worker.developers ?? []).filter(member => !member.return);
  return { key: worker.key, kind: worker.kind, coverage: worker.coverage,
    ...(worker.assignment.work ? { work: worker.assignment.work } : {}),
    agentId: worker.agentId, ...(worker.worktree ? { worktree: worker.worktree } : {}),
    ...(active.length ? { developers: active.map(({ agentId, worktree }) => ({ agentId, worktree })) } : {}),
    ...(worker.developers?.length > active.length ? { retiredDevelopers: worker.developers.length - active.length } : {}),
    ...(worker.permissionPreflights?.length ? { permissionPreflights: worker.permissionPreflights.length } : {}),
    ...(worker.heartbeat ? { heartbeat: worker.heartbeat.status } : {}),
    ...(worker.graph ? { publication: worker.graph.complete ? 'complete' : 'pending' } : {}) };
}

// A settled worker still needs archival, and an owned worktree still needs an
// actual removal or a deliberate retention record, before it leaves the queue.
function retirementView(worker) {
  const phase = !worker.archive ? 'archive-pending'
    : !worker.worktree ? null
      : !worker.cleanup ? 'cleanup-pending'
        : worker.cleanup.removal || worker.cleanup.retention ? null : 'removal-pending';
  if (!phase) return null;
  return { key: worker.key, kind: worker.kind, agentId: worker.agentId,
    ...(worker.worktree ? { worktree: worker.worktree } : {}), phase,
    ...(phase === 'removal-pending'
      ? { recovery: { branch: worker.cleanup.branch, head: worker.cleanup.head } } : {}) };
}

// Current work, not the whole durable record: growing history stays on the board.
export function summarize(state) {
  const pm = state.pm;
  if (!pm) return { initialized: false };
  const open = pm.pending.filter(record => ['pending', 'blocked'].includes(record.status));
  const blockers = (pm.blockers ?? []).filter(episode => !episode.resolution);
  const settled = pm.workers.filter(worker => worker.settled);
  const retirement = settled.map(retirementView).filter(Boolean);
  return {
    mode: pm.mode, ...(pm.config.team ? { team: true } : {}), capacity: pm.config.capacity,
    ...(pm.control?.op === 'suspend' ? { suspension: pm.control } : {}),
    ...(pm.lease ? { lease: { owner: pm.lease.owner, token: pm.lease.token } } : {}),
    ...(pm.schedule ? { schedule: { id: pm.schedule.id, kind: pm.schedule.kind ?? 'schedule',
      cron: pm.schedule.cron, targetAgentId: pm.schedule.targetAgentId, enabled: pm.schedule.enabled } } : {}),
    workers: pm.workers.filter(worker => !worker.settled).map(workerView),
    ...(retirement.length ? { retirement } : {}),
    pending: open.map(({ key, status, evidence }) => ({ key, status, evidence })),
    ...(blockers.length ? { blockers: blockers.map(episode => ({ issue: episode.issue,
      status: episode.status, attempts: episode.attempts.length })) } : {}),
    history: {
      runs: pm.runs.length,
      settledWorkers: settled.length,
      resolvedOperations: pm.pending.length - open.length,
      operationHistory: pm.pending.reduce((total, record) => total + (record.history?.length ?? 0), 0),
      wakeups: pm.wakeupHistory?.length ?? 0, merges: pm.mergeHistory?.length ?? 0,
      blockers: (pm.blockers ?? []).length - blockers.length,
      inspect: '{"op":"inspect","view":"full"}',
    },
  };
}

export function transact(filename, request) {
  requireText(filename, 'board path');
  if (!request || typeof request !== 'object') throw new Error('Invalid request');
  if (existsSync(filename) && !lstatSync(filename).isFile()) throw new Error('Board must be a regular file');
  const read = () => validateState(existsSync(filename) ? JSON.parse(readFileSync(filename, 'utf8')) : {});
  if (request.op === 'inspect') return { status: 'observed', state: read() };
  const lock = `${filename}.write-lock`;
  // No age-based takeover: an abandoned transaction needs explicit, fenced repair.
  mkdirSync(lock, { mode: 0o700 });
  try {
    const state = read();
    const status = apply(state, request);
    validateState(state);
    const next = `${filename}.next`;
    const fd = openSync(next, 'wx', 0o600);
    try {
      writeFileSync(fd, `${JSON.stringify(state, null, 2)}\n`);
      fsyncSync(fd);
    } finally {
      closeSync(fd);
    }
    renameSync(next, filename);
    return { status, state };
  } finally {
    rmdirSync(lock);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const request = JSON.parse(process.argv[3]);
    const view = request?.view ?? 'current';
    if (!['current', 'full'].includes(view)) throw new Error("Unsupported view; use 'current' or 'full'");
    const result = transact(process.argv[2], request);
    process.stdout.write(`${JSON.stringify(view === 'full' ? result
      : { status: result.status, view: summarize(result.state) })}\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
