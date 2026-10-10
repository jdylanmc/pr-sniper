import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const directory = join(dirname(fileURLToPath(import.meta.url)), "..");
const skill = readFileSync(join(directory, "SKILL.md"), "utf8");
const layout = readFileSync(join(directory, "LAYOUT.md"), "utf8");
const runtime = readFileSync(join(directory, "RUNTIME.md"), "utf8");
const intent = readFileSync(join(directory, "intent.md"), "utf8");
const joe = readFileSync(join(directory, "../joe-mode/SKILL.md"), "utf8");
const joeRuntime = readFileSync(
  join(directory, "../joe-mode/RUNTIME.md"),
  "utf8",
);
const worktrees = readFileSync(
  join(directory, "../joe-mode/WORKTREES.md"),
  "utf8",
);
const invocation = readFileSync(
  join(directory, "../setup/INVOCATION.md"),
  "utf8",
);
const squadron = readFileSync(join(directory, "../squadron/SKILL.md"), "utf8");

test("entrypoint is explicitly human-only and session-bound", () => {
  assert.match(skill, /^name: joe-mode-cmux$/m);
  assert.match(skill, /^disable-model-invocation: true$/m);
  assert.match(skill, /^user-invocable: true$/m);
  assert.match(skill, /Human activation only/i);
  assert.match(
    skill,
    /Do not promise work between turns or after the session ends/i,
  );
});

test("adapter preserves one Joe controller and existing delivery policy", () => {
  assert.match(skill, /one logical controller per repository/i);
  assert.match(skill, /not another project-management policy/i);
  assert.match(skill, /Ship, Patch, Refactor, Roast/i);
  assert.match(skill, /merge boundaries/i);
});

test("Maestro activation requires a managed coordinator and actual native tools", () => {
  assert.match(runtime, /launch-settings/);
  assert.match(runtime, /modelPinned/);
  assert.match(runtime, /messagingInstalled/);
  assert.match(runtime, /already a Maestro-managed coordinator/);
  assert.match(
    runtime,
    /Current-session `maestro_peers`, `maestro_send`, and `maestro_spawn`/,
  );
  assert.match(
    runtime,
    /legacy metadata, not evidence of the invoking account/,
  );
  assert.match(
    runtime,
    /Missing checks stop activation before worker creation/,
  );
  assert.match(
    runtime,
    /Running `git -C` or a shell\s+`cd` does not move the invoking conversation/,
  );
  assert.doesNotMatch(runtime, /--require-pinned-launch-settings/);
});

test("layout keeps role areas, developer tabs, worktrees, and chosen icons", () => {
  const diagram = layout.match(/```text\n([\s\S]*?)\n```/)?.[1] ?? "";
  for (const role of [
    "Project Manager",
    "Discovery",
    "Developers",
    "Support",
  ]) {
    assert.match(
      diagram,
      new RegExp(role),
      `${role} must have a prescribed area`,
    );
  }
  assert.match(layout, /developer surface in one developer pane as a tab/i);
  assert.match(layout, /separate Git worktrees/i);
  assert.match(layout, /md-meditation/);
  assert.match(layout, /seti-bicep/);
  assert.match(layout, /md-shield_check_outline/);
});

test("runtime is honest about interaction and continuity", () => {
  assert.match(runtime, /must not use[\s\S]*send-key/i);
  assert.match(
    runtime,
    /controller's `follow-up` subcommand is\s+unsupported for interactive workers/,
  );
  assert.match(runtime, /Use the existing native channel/);
  assert.match(
    runtime,
    /no cron, heartbeat, recurring wake, or unattended pass/i,
  );
  assert.match(runtime, /Restored CMUX panes are visual continuity only/i);
  assert.match(intent, /Restored panes do not prove supervision/i);
});

test("layout scopes Maestro spawn placement and preserves human focus", () => {
  assert.match(layout, /spawn/);
  assert.match(layout, /split-off/);
  assert.match(layout, /move-surface/);
  assert.match(layout, /--focus false/);
  assert.match(layout, /another workspace\/window/i);
  assert.match(layout, /placement capability passes/i);
  assert.match(
    layout,
    /Preserve any existing human-set custom title and color/i,
  );
  assert.match(layout, /cmux rename-tab --surface <returned-surface> "Ready"/);
});

test("workspace presentation uses the correct CMUX command family", () => {
  assert.match(layout, /cmux workspace-action[\s\S]*--action rename/);
  assert.match(
    layout,
    /cmux workspace-action[\s\S]*--action set-color --color Teal/,
  );
});

test("Joe delegates CMUX role launch and messaging to the established framework", () => {
  assert.match(runtime, /\/cmux-maestro-native:cmux-maestro-orchestrate/);
  assert.match(runtime, /global `\/maestro` guide/);
  assert.match(runtime, /maestro_peers/);
  assert.match(runtime, /maestro_send/);
  assert.match(joeRuntime, /active adapter owns every role's runtime/);
  assert.match(
    runtime,
    /Generic harness task\s+IDs are not Maestro worker or peer addresses/,
  );
  assert.match(
    runtime,
    /Do not\s+inspect private bindings, invoke proof fixtures/,
  );
  assert.doesNotMatch(
    runtime,
    /does not supply[\s\S]{0,80}machine-readable worker/,
  );
});

test("native registration is not messaging adoption or automatic repair", () => {
  assert.match(
    runtime,
    /Registration alone does\s+not make a coordinator a messaging recipient/,
  );
  assert.match(
    runtime,
    /Do not continue through human\s+relay, hidden SDK helpers/,
  );
  assert.match(
    runtime,
    /Do not\s+restart, adopt, replace, or spawn a new coordinator merely to obtain an address/,
  );
  assert.match(
    runtime,
    /Starting a new root requires the human's separate direction/,
  );
  assert.match(
    runtime,
    /Do not\s+install or refresh global skills automatically/,
  );
  assert.match(
    invocation,
    /Registration alone does\s+not make a coordinator a messaging recipient/,
  );
});

test("peer routing and delivery limits remain distinct from Joe acceptance", () => {
  for (const field of ["workspaceId", "sessionId", "generation"]) {
    assert.ok(runtime.includes(`\`${field}\``), field);
  }
  assert.match(runtime, /4096 UTF-8 bytes/);
  assert.match(runtime, /received envelope's exact\s+`sender`/);
  assert.match(
    runtime,
    /local\s+write attempt; delivery and completion are unconfirmed/,
  );
  assert.match(runtime, /No automatic retries/);
  assert.match(runtime, /No automatic retries,[\s\S]*custom busy scheduler/);
  assert.match(
    runtime,
    /not accepted work,\s+human approval, or custody transfer/,
  );
  assert.match(runtime, /Do not\s+inspect private bindings/);
  assert.match(
    runtime,
    /independent of visual focus, app activation, and sidebar\s+visibility/,
  );
});

test("YOLO is explicit coordinator-only and does not invent inherited grants", () => {
  assert.match(runtime, /explicit human-approved coordinator-only/);
  assert.match(runtime, /Native `yolo: true`/);
  assert.match(runtime, /preserving denies/);
  assert.match(runtime, /never a default,\s+inferred permission inheritance/);
  assert.match(runtime, /Worker actors cannot\s+request YOLO for descendants/);
});

test("unsupported layout and proposed lifecycle do not widen capabilities", () => {
  assert.match(
    runtime,
    /established lifecycle guide does not authorize arbitrary pane placement/,
  );
  assert.match(runtime, /matching sentence alone is not\s+runtime proof/);
  assert.match(layout, /do not patch\s+the installed guide or bypass its rule/);
  assert.match(
    runtime,
    /proposed Roster\/Stage exit-and-close UX is not an installed lifecycle/,
  );
});

test("all nested roles retain Maestro, and generic Joe does not implement its internals", () => {
  assert.match(runtime, /Every role uses `maestro_spawn`/);
  assert.match(runtime, /reviewer,[\s\S]*blocker investigator/);
  assert.match(runtime, /runtime: Maestro/);
  assert.match(
    runtime,
    /Ship, Squadron, or a generic background example cannot change that runtime/,
  );
  assert.match(skill, /Never substitute an SDK\/task agent/);
  assert.match(squadron, /inherited runtime-adapter contract takes precedence/);
  assert.match(joeRuntime, /Runtime mechanics belong in the selected/);
  assert.doesNotMatch(
    joeRuntime,
    /CMUX_MAESTRO_|maestro_spawn|maestro_send|private bindings/,
  );
  assert.match(joeRuntime, /retired archive or different package/);
});

test("running counts require more than supervisor acknowledgement or prepared work", () => {
  assert.match(
    runtime,
    /supervisor acknowledgement does not prove provider startup or adapter/,
  );
  assert.match(runtime, /`messaging: configured` is not messaging readiness/);
  assert.match(
    runtime,
    /SDK task IDs, labels, idle state, and failed tabs are not running agents/,
  );
  assert.match(runtime, /stop fan-out and reconcile retained resources/);
  assert.match(skill, /including PM/);
});

test("both entrypoints bind PM, Discovery and merger to distinct role worktrees", () => {
  assert.match(joe, /\[role worktree placement\]\(WORKTREES.md\)/);
  assert.match(skill, /\[Joe role worktrees\]\(\.\.\/joe-mode\/WORKTREES.md\)/);
  for (const text of [joe, skill, runtime, layout, invocation, worktrees]) {
    for (const roleBranch of ["main", "discovery/<feat>", "pr-sniper"]) {
      assert.ok(text.includes(`\`${roleBranch}\``), `missing ${roleBranch}`);
    }
  }
  assert.match(worktrees, /Every repository-backed Discovery agent/);
  assert.match(worktrees, /even\s+when its current pass is read-only/);
  assert.match(
    worktrees,
    /All cockpit roles remain in the existing CMUX workspace/,
  );
  assert.match(worktrees, /active adapter's public\s+launch interface/);
  assert.match(worktrees, /Placement grants no merge permission/);
});

test("main advancement is guarded and never mutates another active writer", () => {
  assert.match(
    worktrees,
    /At activation, before a new dispatch pass, and after each confirmed merge/,
  );
  assert.match(worktrees, /fetch origin main/);
  assert.match(worktrees, /merge --ff-only refs\/remotes\/origin\/main/);
  assert.match(worktrees, /clean\s+\(including untracked work\)/);
  assert.match(worktrees, /local `main` is an ancestor/);
  assert.match(worktrees, /local main is ahead or diverged, dirty/);
  assert.match(worktrees, /Never reset, stash, rebase, force-checkout/);
  assert.match(worktrees, /must not rebase another active writer's worktree/);
  assert.match(worktrees, /Verify branch and resulting tip/);
  assert.match(
    worktrees,
    /reconcile that checkout rather than forcing a second checkout/,
  );
});
