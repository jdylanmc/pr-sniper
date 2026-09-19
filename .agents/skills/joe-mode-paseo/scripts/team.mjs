import { isDeepStrictEqual } from 'node:util';

export const teamKinds = ['delivery', 'discovery', 'research', 'shepherd', 'coordinator', 'roast', 'investigator'];
const singletonKinds = ['discovery', 'shepherd', 'coordinator'];
const workSlots = { feature: 2, bug: 1, hardening: 1, refactor: 1 };

function text(value, label) {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`Missing ${label}`);
}

export function developerSlots(assignment) {
  if (assignment.kind !== 'delivery') return 0;
  if (!Object.hasOwn(workSlots, assignment.work)) throw new Error('Invalid delivery work kind');
  return workSlots[assignment.work];
}

function permissionSnapshot(snapshot) {
  text(snapshot?.provider, 'permission provider');
  text(snapshot.modeId, 'permission mode');
  if (!snapshot.features || typeof snapshot.features !== 'object' || Array.isArray(snapshot.features)) {
    throw new Error('Missing permission features');
  }
}

function validatePreflight(plan) {
  for (const key of ['launchId', 'parentAgentId', 'workspaceId', 'authority', 'evidence']) {
    text(plan[key], `permission preflight ${key}`);
  }
  if (!['bind', 'staff'].includes(plan.purpose)) throw new Error('Invalid permission preflight purpose');
  // The child agent cannot exist yet: plan the exact workspace and worktree instead.
  if (plan.purpose === 'staff' || plan.worktree !== undefined) text(plan.worktree, 'permission preflight worktree');
  permissionSnapshot(plan.parent);
  permissionSnapshot(plan.target);
  if (plan.launch !== undefined) {
    text(plan.launch?.agentId, 'permission launch agent');
    text(plan.launch.evidence, 'permission launch receipt');
  }
  if (plan.parent.provider === plan.target.provider) {
    if (!isDeepStrictEqual(plan.parent, plan.target)) throw new Error('Same-provider permission mismatch');
    return;
  }
  const mapping = plan.mapping;
  if (!['equivalent', 'authorized-mapping'].includes(mapping?.kind)) throw new Error('Missing permission mapping');
  if (mapping.preservesChoices !== true) throw new Error('Permission mapping must preserve human choices');
  for (const key of ['sourceCapabilities', 'targetCapabilities', 'rationale', 'evidence']) {
    text(mapping[key], `permission mapping ${key}`);
  }
  if (mapping.kind === 'authorized-mapping') text(mapping.authority, 'permission mapping authority');
}

export function permissionProof(proof, preflights = [], purpose = 'bind', launch = {}) {
  text(proof?.authority, 'permission authority');
  text(proof.evidence, 'permission readback');
  permissionSnapshot(proof.parent);
  permissionSnapshot(proof.child);
  if (proof.parent.provider === proof.child.provider) {
    if (!isDeepStrictEqual(proof.parent, proof.child)) throw new Error('Child permission mismatch');
    if (!proof.preflight) return;
  }
  const plan = preflights.find(item => item.launchId === proof.preflight);
  if (!plan) throw new Error('Missing recorded permission preflight before launch');
  validatePreflight(plan);
  for (const [actual, expected] of [
    [proof.parent, plan.parent], [proof.child, plan.target], [proof.authority, plan.authority],
    [proof.parentAgentId, plan.parentAgentId], [proof.workspaceId, plan.workspaceId], [purpose, plan.purpose],
  ]) {
    if (!isDeepStrictEqual(actual, expected)) {
      throw new Error('Permission preflight drift; hold work and reconcile current parent/target');
    }
  }
  if (!plan.launch) throw new Error('Missing actual launch receipt for this permission preflight');
  if (plan.launch.agentId !== launch.agentId || plan.worktree !== launch.worktree) {
    throw new Error('Permission preflight belongs to another launch, agent or worktree');
  }
}

function episode(pm, issue) {
  return pm.blockers?.find(item => item.issue === issue && !item.resolution);
}

function activeDevelopers(worker) {
  return (worker.developers ?? []).filter(member => !member.return);
}

function participants(worker) {
  return [{ agentId: worker.agentId, worktree: worker.worktree },
    ...(worker.developers ?? []).map(({ agentId, worktree }) => ({ agentId, worktree }))];
}

function attemptParticipants(pm, attempt) {
  // Derive from the preserved worker: a lane may still staff more developers while
  // one covered issue is blocked, and every one of them stays barred from the retry.
  const worker = pm.workers.find(item => item.key === attempt.key);
  if (!worker) throw new Error('Missing blocker worker; reconcile participant history');
  return participants(worker);
}

function checkFreshAttempt(pm, worker, request) {
  for (const issue of worker.coverage) {
    const blocked = episode(pm, issue);
    if (!blocked) continue;
    if (blocked.status === 'blocked') throw new Error(`Issue blocked: ${issue}`);
    for (const attempt of blocked.attempts) {
      if (attemptParticipants(pm, attempt).some(member =>
        member.agentId === request.agentId || member.worktree === request.worktree)) {
        throw new Error('Retry needs fresh context and worktree for every participant');
      }
    }
  }
}

export function checkTeamReservation(pm, worker) {
  developerSlots(worker);
  const active = pm.workers.filter(item => !item.settled);
  if (singletonKinds.includes(worker.kind) && active.some(item => item.kind === worker.kind)) {
    throw new Error(`${worker.kind} already reserved`);
  }
  if (worker.kind === 'coordinator' && pm.config.merge !== 'orchestrator') {
    throw new Error('PR coordinator needs the repository merge grant');
  }
  if (worker.kind !== 'delivery') return;
  const used = active.reduce((sum, item) => sum + developerSlots(item.assignment), 0);
  if (used + developerSlots(worker) > pm.config.capacity) throw new Error('Developer capacity exhausted');
  for (const issue of worker.coverage) {
    const blocked = episode(pm, issue);
    if (!blocked) continue;
    if (blocked.status === 'blocked') throw new Error(`Issue blocked: ${issue}`);
    const previous = pm.workers.find(item => item.key === blocked.attempts.at(-1).key);
    if (!previous?.settled || !previous.archive) throw new Error('Retire previous blocked attempt before retry');
  }
}

export function checkTeamBinding(pm, worker, request) {
  permissionProof(request.permissions, worker.permissionPreflights, 'bind',
    { agentId: request.agentId, worktree: worker.kind === 'delivery' ? request.worktree : undefined });
  if (pm.workers.some(other => other !== worker && !other.settled &&
    (other.agentId === request.agentId || activeDevelopers(other).some(member => member.agentId === request.agentId)))) {
    throw new Error('Agent already owns another role');
  }
  if (request.agentId === pm.config.pmAgentId) throw new Error('PM cannot be its own worker');
  if (worker.kind !== 'delivery') return;
  text(request.worktree, 'observed delivery worktree');
  if (worker.worktree && worker.worktree !== request.worktree) throw new Error('Changed bound worktree');
  checkFreshAttempt(pm, worker, request);
}

// Preservation, removal and retention describe one custody outcome each.
function checkCleanupRecord(cleanup) {
  if (typeof cleanup !== 'object' || cleanup === null || Array.isArray(cleanup)) throw new Error('Invalid cleanup record');
  if (cleanup.retention !== undefined) {
    text(cleanup.retention, 'cleanup retention');
    if (['branch', 'head', 'preservation', 'removal'].some(key => cleanup[key] !== undefined)) {
      throw new Error('Contradictory cleanup record');
    }
    return;
  }
  text(cleanup.branch, 'cleanup recovery branch');
  text(cleanup.head, 'cleanup recovery head');
  text(cleanup.preservation, 'cleanup preservation');
  if (cleanup.removal !== undefined) text(cleanup.removal, 'cleanup removal');
}

export function checkTeamState(pm) {
  if (pm.blockers !== undefined && !Array.isArray(pm.blockers)) throw new Error('Invalid blocker history');
  const unresolved = new Set();
  for (const current of pm.blockers ?? []) {
    text(current.issue, 'blocker issue');
    if (!Array.isArray(current.attempts) || current.attempts.length < 1 || current.attempts.length > 2 ||
      !['retry', 'blocked'].includes(current.status)) throw new Error('Invalid blocker episode');
    const attempts = new Set();
    const agents = new Set();
    const trees = new Set();
    const investigators = new Set();
    for (const attempt of current.attempts) {
      for (const key of ['key', 'agentId', 'worktree', 'investigator', 'selfReview', 'challenge', 'missing', 'evidence']) {
        text(attempt[key], `blocker ${key}`);
      }
      if (!['work', 'permission', 'human'].includes(attempt.category) || attempts.has(attempt.key) ||
        agents.has(attempt.agentId) || trees.has(attempt.worktree) || investigators.has(attempt.investigator) ||
        attempt.agentId === attempt.investigator) throw new Error('Invalid independent blocker attempts');
      attempts.add(attempt.key);
      agents.add(attempt.agentId);
      trees.add(attempt.worktree);
      investigators.add(attempt.investigator);
      const members = attemptParticipants(pm, attempt);
      if (!members.some(member => member.agentId === attempt.agentId && member.worktree === attempt.worktree)) {
        throw new Error('Invalid blocker participant history');
      }
      for (const member of members) {
        text(member.agentId, 'blocker participant agent');
        text(member.worktree, 'blocker participant worktree');
        if (member.agentId === attempt.investigator) throw new Error('Invalid independent blocker lens');
      }
      for (const previous of current.attempts.slice(0, current.attempts.indexOf(attempt))) {
        if (attemptParticipants(pm, previous).some(prior => members.some(member =>
          member.agentId === prior.agentId || member.worktree === prior.worktree))) {
          throw new Error('Invalid fresh blocker participants');
        }
      }
    }
    const blocked = current.attempts.length === 2 || current.attempts.some(item => item.category !== 'work');
    if (current.status !== (blocked ? 'blocked' : 'retry')) throw new Error('Invalid blocker status');
    if (current.resolution) {
      text(current.resolution.evidence, 'blocker resolution');
      text(current.resolution.readiness, 'resolved readiness');
      if (current.attempts.some(item => item.category !== 'work')) text(current.resolution.human, 'human unblock decision');
    } else {
      if (unresolved.has(current.issue)) throw new Error('Duplicate unresolved blocker');
      unresolved.add(current.issue);
    }
  }
  let used = 0;
  const singletons = new Set();
  const agents = new Set();
  const developers = new Set();
  const worktrees = new Set();
  for (const worker of pm.workers) {
    if (worker.permissionPreflights !== undefined) {
      if (!Array.isArray(worker.permissionPreflights)) throw new Error('Invalid permission preflight history');
      const launches = new Set();
      for (const plan of worker.permissionPreflights) {
        validatePreflight(plan);
        if (launches.has(plan.launchId)) throw new Error('Duplicate permission preflight');
        launches.add(plan.launchId);
      }
    }
    if (worker.settled && worker.heartbeat && !['deleted', 'absent'].includes(worker.heartbeat.status)) {
      throw new Error('Settled role has an unresolved heartbeat');
    }
    if (worker.cleanup !== undefined) checkCleanupRecord(worker.cleanup);
    // Quiescent migration preserves old, settled assignments in their original units.
    if (worker.settled) continue;
    used += developerSlots(worker.assignment);
    if (singletonKinds.includes(worker.kind)) {
      if (singletons.has(worker.kind)) throw new Error('Duplicate team role');
      singletons.add(worker.kind);
    }
    if (worker.agentId) {
      if (agents.has(worker.agentId) || worker.agentId === pm.config.pmAgentId ||
        pm.workers.some(other => other !== worker && !other.settled &&
          activeDevelopers(other).some(member => member.agentId === worker.agentId))) throw new Error('Duplicate role agent');
      agents.add(worker.agentId);
      permissionProof(worker.permissions, worker.permissionPreflights, 'bind',
        { agentId: worker.agentId, worktree: worker.worktree });
      if (worker.kind === 'delivery') text(worker.worktree, 'delivery worktree');
    }
    if (activeDevelopers(worker).length > developerSlots(worker.assignment)) throw new Error('Invalid developer capacity');
    for (const member of worker.developers ?? []) {
      text(member.agentId, 'developer agent');
      text(member.worktree, 'developer worktree');
      permissionProof(member.permissions, worker.permissionPreflights, 'staff',
        { agentId: member.agentId, worktree: member.worktree });
      if (member.return) {
        for (const key of ['evidence', 'result', 'acceptance', 'archive']) text(member.return[key], `developer return ${key}`);
        continue;
      }
      if (developers.has(member.agentId) || worktrees.has(member.worktree)) throw new Error('Duplicate developer or worktree');
      developers.add(member.agentId);
      worktrees.add(member.worktree);
    }
    const heartbeat = worker.heartbeat;
    if (heartbeat) {
      if (!['shepherd', 'discovery'].includes(worker.kind) ||
        !['pending', 'active', 'uncertain', 'deleted', 'absent'].includes(heartbeat.status)) throw new Error('Invalid role heartbeat');
      text(heartbeat.settings, 'heartbeat settings');
      text(heartbeat.evidence, 'heartbeat evidence');
      if (heartbeat.status === 'active' || heartbeat.status === 'deleted') text(heartbeat.id, 'heartbeat ID');
      if (heartbeat.status === 'absent') text(heartbeat.absence, 'verified heartbeat absence');
    }
  }
  if (used > pm.config.capacity) throw new Error('Invalid developer capacity');
}

function roleHeartbeat(pm, worker, request) {
  if (!['shepherd', 'discovery'].includes(worker.kind) || !worker.agentId || worker.settled) {
    throw new Error('Heartbeat needs a live bound persistent role');
  }
  const previous = worker.heartbeat;
  if (request.action === 'plan') {
    if (pm.mode !== 'enabled') throw new Error('PM is not enabled');
    if (previous && !['deleted', 'absent'].includes(previous.status)) throw new Error('Reconcile existing heartbeat');
    text(request.settings, 'approved heartbeat settings');
    worker.heartbeatHistory ??= [];
    if (previous) worker.heartbeatHistory.push(previous);
    worker.heartbeat = { status: 'pending', settings: request.settings, evidence: request.evidence };
  } else if (request.action === 'created') {
    if (!previous || !['pending', 'uncertain'].includes(previous.status) || previous.id) {
      throw new Error('Missing heartbeat creation intent');
    }
    text(request.id, 'heartbeat ID');
    if (request.targetAgentId !== worker.agentId) throw new Error('Wrong heartbeat target');
    if (pm.schedule?.id === request.id || pm.workers.some(other => other !== worker && other.heartbeat?.id === request.id)) {
      throw new Error('Heartbeat already owned');
    }
    // A late receipt after pause is still recorded so its exact job can be deleted.
    worker.heartbeat = { ...previous, id: request.id, status: 'active', evidence: request.evidence };
  } else {
    if (!previous || ['deleted', 'absent'].includes(previous.status) ||
      (previous.id && request.id !== previous.id)) throw new Error('Wrong or missing owned heartbeat');
    if (request.action === 'absent') {
      text(request.absence, 'verified heartbeat absence');
    } else if (request.action === 'deleted') {
      text(request.id, 'heartbeat ID');
      if (!previous.id) throw new Error('Reconcile unknown heartbeat before deletion receipt');
    } else if (!['observed', 'uncertain'].includes(request.action)) {
      throw new Error('Invalid heartbeat action');
    }
    if (request.action === 'observed' && previous.status !== 'active') throw new Error('No active heartbeat to observe');
    worker.heartbeat = { ...previous, status: request.action === 'observed' ? 'active' : request.action,
      evidence: request.evidence, ...(request.action === 'absent' ? { absence: request.absence } : {}) };
  }
  return 'heartbeat-recorded';
}

function block(pm, worker, request) {
  if (worker.kind !== 'delivery' || !worker.agentId || worker.settled || !worker.coverage.includes(request.issue)) {
    throw new Error('Blocker needs an active bound delivery covering the issue');
  }
  for (const key of ['investigator', 'selfReview', 'challenge', 'missing']) text(request[key], key);
  if (request.investigator === worker.agentId ||
    worker.developers?.some(member => member.agentId === request.investigator)) throw new Error('Need an independent blocker lens');
  if (!['work', 'permission', 'human'].includes(request.category)) throw new Error('Invalid blocker category');
  pm.blockers ??= [];
  let current = episode(pm, request.issue);
  if (!current) {
    current = { issue: request.issue, attempts: [], status: 'retry' };
    pm.blockers.push(current);
  }
  const attempt = { key: worker.key, agentId: worker.agentId, worktree: worker.worktree,
    investigator: request.investigator, selfReview: request.selfReview, challenge: request.challenge,
    missing: request.missing, category: request.category, evidence: request.evidence };
  const prior = current.attempts.find(item => item.key === worker.key);
  if (prior) {
    if (!isDeepStrictEqual(prior, attempt)) throw new Error('Changed blocker attempt; reconcile existing evidence');
    return current.status;
  }
  if (current.status === 'blocked') throw new Error('Issue already blocked');
  if (current.attempts.some(item => item.agentId === attempt.agentId || item.worktree === attempt.worktree ||
    item.investigator === attempt.investigator)) throw new Error('Retry needs independent contexts and blocker lenses');
  current.attempts.push(attempt);
  current.status = request.category !== 'work' || current.attempts.length >= 2 ? 'blocked' : 'retry';
  return current.status;
}

export function teamOperation(pm, request) {
  if (!pm.config.team) throw new Error('Enable team mode through paused reconciliation first');
  if (request.op === 'unblock') {
    const current = episode(pm, request.issue);
    if (!current) throw new Error('No unresolved blocker');
    text(request.resolution, 'blocker resolution');
    text(request.readiness, 'verified readiness');
    if (current.attempts.some(item => item.category !== 'work')) text(request.human, 'human decision or grant');
    current.resolution = { evidence: request.resolution, readiness: request.readiness, human: request.human };
    return 'unblocked';
  }
  const worker = pm.workers.find(item => item.key === request.key);
  if (!worker) throw new Error('Unknown worker');
  text(request.evidence, 'live evidence');
  if (request.op === 'permission-preflight') {
    if (pm.mode !== 'enabled' || worker.settled) throw new Error('Permission preflight needs enabled live reservation');
    const plan = Object.fromEntries(['launchId', 'parentAgentId', 'workspaceId', 'worktree', 'parent',
      'target', 'authority', 'evidence', 'mapping'].filter(key => request[key] !== undefined)
      .map(key => [key, request[key]]));
    plan.purpose = request.purpose ?? 'bind';
    if (plan.purpose === 'bind' && worker.kind === 'delivery') text(plan.worktree, 'permission preflight worktree');
    validatePreflight(plan);
    worker.permissionPreflights ??= [];
    const previous = worker.permissionPreflights.find(item => item.launchId === plan.launchId);
    if (previous) {
      const { launch, ...recorded } = previous;
      if (!isDeepStrictEqual(recorded, plan)) throw new Error('Changed permission preflight; reconcile and record a new launch intent');
      return 'permission-preflight-recorded';
    }
    if (plan.purpose === 'bind' && worker.agentId) throw new Error('Worker already bound; preflight must precede launch');
    if (plan.purpose === 'staff' && (!worker.agentId || worker.kind !== 'delivery')) {
      throw new Error('Developer preflight needs a bound delivery');
    }
    worker.permissionPreflights.push(plan);
    return 'permission-preflight-recorded';
  }
  if (request.op === 'permission-launch') {
    if (pm.mode !== 'enabled' || worker.settled) throw new Error('Permission launch needs enabled live reservation');
    const plan = worker.permissionPreflights?.find(item => item.launchId === request.launchId);
    if (!plan) throw new Error('Unknown permission preflight');
    // The plan is intent; this records the child the runtime actually created for it.
    text(request.agentId, 'permission launch agent');
    if (request.workspaceId !== plan.workspaceId || request.worktree !== plan.worktree) {
      throw new Error('Launched placement differs from the recorded permission preflight');
    }
    const launch = { agentId: request.agentId, evidence: request.evidence };
    if (plan.launch) {
      if (!isDeepStrictEqual(plan.launch, launch)) throw new Error('Changed permission launch receipt; record a new launch intent');
      return 'permission-launch-recorded';
    }
    if (worker.permissionPreflights.some(item => item.launch?.agentId === request.agentId)) {
      throw new Error('Launch receipt already recorded for that agent');
    }
    plan.launch = launch;
    return 'permission-launch-recorded';
  }
  if (request.op === 'role-heartbeat') return roleHeartbeat(pm, worker, request);
  if (request.op === 'block') return block(pm, worker, request);
  if (request.op === 'retire-developer') {
    const member = worker.developers?.find(item => item.agentId === request.agentId);
    if (!member) throw new Error('Unknown developer');
    if (request.noLiveWriters !== true || request.noUntransferredDuties !== true) throw new Error('Unreconciled developer custody');
    for (const key of ['result', 'acceptance', 'archive']) text(request[key], `developer ${key}`);
    const returned = { evidence: request.evidence, result: request.result,
      acceptance: request.acceptance, archive: request.archive };
    if (member.return && !isDeepStrictEqual(member.return, returned)) throw new Error('Changed developer return');
    member.return = returned;
    return 'developer-retired';
  }
  if (request.op === 'staff') {
    if (pm.mode !== 'enabled' || worker.settled || !worker.agentId || worker.kind !== 'delivery') {
      throw new Error('Staffing needs an enabled bound delivery');
    }
    text(request.agentId, 'developer agent');
    text(request.worktree, 'developer worktree');
    checkFreshAttempt(pm, worker, request);
    permissionProof(request.permissions, worker.permissionPreflights, 'staff',
      { agentId: request.agentId, worktree: request.worktree });
    worker.developers ??= [];
    const member = { agentId: request.agentId, worktree: request.worktree,
      permissions: request.permissions, evidence: request.evidence };
    const previous = worker.developers.find(item => item.agentId === request.agentId);
    if (previous) {
      if (!isDeepStrictEqual(previous, member)) throw new Error('Changed developer binding');
      return 'staffed';
    }
    for (const other of pm.workers.filter(item => !item.settled)) {
      if (other !== worker && other.agentId === request.agentId) throw new Error('Developer owns another role');
      if (activeDevelopers(other).some(item => item.agentId === request.agentId || item.worktree === request.worktree)) {
        throw new Error('Developer or worktree already staffed');
      }
    }
    if (request.agentId === pm.config.pmAgentId) throw new Error('PM is not a developer');
    if (activeDevelopers(worker).length >= developerSlots(worker.assignment)) throw new Error('Developer capacity exhausted');
    worker.developers.push(member);
    return 'staffed';
  }
  if (!worker.settled || !worker.archive) throw new Error('Cleanup needs settled and archived custody');
  if (request.op === 'cleanup-ready') {
    if (worker.cleanup?.retention) throw new Error('Worktree recorded as retained; reconcile custody before removal');
    if (worker.cleanup?.removal) throw new Error('Worktree already removed; that outcome stands');
    if (request.noLiveWriters !== true || request.clean !== true) throw new Error('Live writers or unpreserved files');
    text(request.branch, 'remote recovery branch');
    if (!/^refs\/heads\/.+/.test(request.branch) || !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(request.localHead) ||
      request.localHead !== request.remoteHead) throw new Error('Unverified remote preservation');
    const preserved = { branch: request.branch, head: request.localHead, preservation: request.evidence };
    // An accepted receipt is the record of what actually happened: never rewrite it.
    if (worker.cleanup) {
      if (!isDeepStrictEqual(worker.cleanup, preserved)) {
        throw new Error('Changed cleanup preservation receipt; reconcile custody');
      }
      return 'cleanup-ready';
    }
    worker.cleanup = preserved;
    return 'cleanup-ready';
  }
  if (request.op === 'cleanup') {
    // Keeping an owned worktree is a real outcome, not a skipped deletion.
    if (request.retained === true) {
      if (worker.cleanup?.removal) throw new Error('Worktree already removed; that outcome stands');
      if (worker.cleanup?.retention) {
        if (worker.cleanup.retention !== request.evidence) throw new Error('Changed cleanup retention receipt; reconcile custody');
        return 'retained';
      }
      if (worker.cleanup) throw new Error('Verified removal preparation recorded; reconcile custody');
      worker.cleanup = { retention: request.evidence };
      return 'retained';
    }
    if (!worker.cleanup || worker.cleanup.retention) throw new Error('Missing verified remote preservation');
    if (worker.cleanup.removal !== undefined) {
      if (worker.cleanup.removal !== request.evidence) throw new Error('Changed cleanup removal receipt; reconcile custody');
      return 'cleaned';
    }
    worker.cleanup.removal = request.evidence;
    return 'cleaned';
  }
  throw new Error('Unsupported team operation');
}
