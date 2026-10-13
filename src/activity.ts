import type { MonitoringSnapshot } from "./monitoring";
import { keyValues, type KeyValue } from "./key-values";
import { compactDuration, openDestination, type QueueItem } from "./queue";

const paths = {
  review:
    "M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12Zm10-3a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z",
  findings: "M5 21V4m0 0h11l-2 4 2 4H5",
  comment: "M4 5h16v11H9l-5 4V5Z",
  reply: "M9 7 4 12l5 5M4 12h11a5 5 0 0 1 5 5v1",
  final: "M12 3 5 6v6c0 4 3 7 7 9 4-2 7-5 7-9V6l-7-3Zm-3 9 2 2 4-4",
  approve: "M5 12.5 10 17.5 19 7",
  merge: "M6 3v12m0 0a3 3 0 1 0 .01 0M18 9a3 3 0 1 0 .01 0M18 12c0 3-3 3-12 3",
  retry: "M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5",
  open: "M10 6H6a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4M14 4h6v6m0-6-9 9",
};

function icon(path: string) {
  return `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
}

interface Row {
  kind: string;
  icon: string;
  verb: string;
  value: string;
  at?: number | null;
  agent?: string;
  open?: boolean;
  facts: KeyValue[];
}

const state = (value: string) => value.replaceAll("_", " ");
// Informational count of files read; never a gate.
const coverageFacts = (
  coverage: { read: number; changed: number } | null | undefined,
): KeyValue[] =>
  coverage ? [["Read", `${coverage.read} of ${coverage.changed}`]] : [];
const short = (value: string | null | undefined) => (value ?? "").slice(0, 7);

export function activityRows(
  item: QueueItem,
  snapshot: MonitoringSnapshot,
): Row[] {
  const rows: Row[] = [];
  const head = item.job.head_sha;
  const candidates = (snapshot.reviews ?? []).filter(
    (candidate) =>
      item.review_keys.includes(candidate.key) &&
      candidate.run &&
      (candidate.job?.head_sha ?? head) === head,
  );
  const operations = new Set<string>();
  for (const candidate of candidates) {
    const run = candidate.run!;
    const agent = run.selection.agent.name ?? candidate.agent_name;
    operations.add(run.operation.id);
    const result = run.result;
    if (result) {
      const findings = result.output.findings;
      const high = findings.filter((finding) =>
        ["high", "critical"].includes(finding.severity.toLowerCase()),
      ).length;
      rows.push({
        kind: "review",
        icon: paths.review,
        verb: "Reviewed",
        value: `${result.output.files.length} files`,
        at: run.operation.initial_attempt_at,
        agent,
        facts: [
          ["Agent", agent],
          ["Model", result.model],
          [
            "Effort",
            run.selection.agent.intelligence?.reasoning_effort ?? "Default",
          ],
          ["Tokens", `${result.input_tokens} / ${result.output_tokens}`],
          ["Tools", String(result.tool_calls)],
          ...coverageFacts(result.coverage),
          ["Session", result.session_id, true],
          ["Decision", state(result.output.decision)],
          ["Revision", short(head)],
          ["Run", run.operation.id, true],
        ],
      });
      rows.push({
        kind: "findings",
        icon: paths.findings,
        verb: "Findings",
        value: findings.length
          ? high
            ? `${findings.length} \u00b7 ${high} high`
            : String(findings.length)
          : "0",
        agent,
        facts: [
          ["Count", String(findings.length)],
          ["High", String(high)],
          ["Held", String(result.output.held_findings?.length ?? 0)],
        ],
      });
    }
    if (run.operation.attempt_count > 1)
      rows.push({
        kind: "retried",
        icon: paths.retry,
        verb: "Retried",
        value: `${run.operation.attempt_count - 1}x`,
        agent,
        facts: [
          ["Attempts", String(run.operation.attempt_count)],
          ["State", state(run.operation.state)],
          ["Run", run.operation.id, true],
        ],
      });
  }
  for (const publication of snapshot.publications ?? []) {
    const record = publication.publication;
    if (!record || !operations.has(publication.review_operation_id)) continue;
    const posted = record.receipts.filter(
      (receipt) => receipt.state === "commented",
    );
    if (!posted.length) continue;
    const comments = posted.reduce(
      (sum, receipt) => sum + receipt.comment_ids.length,
      0,
    );
    rows.push({
      kind: "commented",
      icon: paths.comment,
      verb: "Commented",
      value: `${comments} inline`,
      open: true,
      facts: [
        ["Phase", state(record.phase)],
        ...posted.map((receipt, index): KeyValue => [
          posted.length > 1 ? `Review ${index + 1}` : "Review",
          receipt.review_id,
          true,
        ]),
        ...posted.flatMap((receipt) =>
          receipt.comment_ids.map((id, index): KeyValue => [
            `Comment ${index + 1}`,
            id,
            true,
          ]),
        ),
      ],
    });
  }
  for (const reply of snapshot.follow_ups ?? []) {
    if (!item.follow_up_ids.includes(reply.run.id) || !reply.run.receipt)
      continue;
    rows.push({
      kind: "replied",
      icon: paths.reply,
      verb: "Replied",
      value: reply.run.target?.kind === "mention" ? "mention" : "thread",
      open: true,
      facts: [
        ["Reply", reply.run.receipt, true],
        ["Run", reply.run.id, true],
        ["Phase", state(reply.run.phase)],
      ],
    });
  }
  const status = item.action_status;
  const final = status?.final_review;
  if (final) {
    const execution = final.execution;
    const result = execution.result;
    const agent = execution.selection.agent.name;
    rows.push({
      kind: "final",
      icon: paths.final,
      verb: "Final",
      value: state(
        final.cancelled ? "cancelled" : execution.operation.state,
      ).replace("completed", "done"),
      at: execution.operation.initial_attempt_at,
      agent,
      facts: [
        ["Agent", agent],
        ["Model", result?.model ?? execution.selection.agent.model ?? ""],
        [
          "Effort",
          execution.selection.agent.intelligence?.reasoning_effort ?? "Default",
        ],
        [
          "Tokens",
          result ? `${result.input_tokens} / ${result.output_tokens}` : "",
        ],
        ["Tools", result ? String(result.tool_calls) : ""],
        ...coverageFacts(result?.coverage),
        ["Session", result?.session_id ?? "", true],
        ["Files", result ? String(result.output.files.length) : ""],
        ["Order", String(final.enqueue_order)],
        ["Run", execution.operation.id, true],
      ],
    });
    if (execution.operation.attempt_count > 1)
      rows.push({
        kind: "retried",
        icon: paths.retry,
        verb: "Retried",
        value: `${execution.operation.attempt_count - 1}x`,
        agent,
        facts: [["Attempts", String(execution.operation.attempt_count)]],
      });
  }
  let approved = false;
  for (const effect of status?.effects ?? []) {
    const confirmed = effect.state === "confirmed" && effect.receipt;
    const merge = effect.action === "merge";
    if (!merge && confirmed) approved = true;
    rows.push({
      kind: effect.action,
      icon: merge ? paths.merge : paths.approve,
      verb: merge ? "Merged" : "Approved",
      value: confirmed
        ? merge
          ? short(effect.receipt!.merge_commit)
          : short(effect.receipt!.head)
        : state(effect.cancelled ? "cancelled" : effect.state),
      open: !!confirmed,
      facts: [
        ["State", state(effect.state)],
        ["Receipt", effect.receipt?.id ?? "None", !!effect.receipt],
        ["Actor", effect.receipt?.actor_id ?? ""],
        ["Head", effect.receipt?.head ?? "", !!effect.receipt],
        ["Merge", effect.receipt?.merge_commit ?? "", !!effect.receipt],
        ["Error", effect.error ?? ""],
      ],
    });
  }
  const peer = status?.provider_approval;
  if (peer && !approved)
    rows.push({
      kind: "approve",
      icon: paths.approve,
      verb: "Approved",
      value: short(peer.head),
      open: true,
      facts: [
        ["Receipt", peer.id, true],
        ["Actor", peer.actor_id],
        ["Head", peer.head, true],
      ],
    });
  return rows;
}

export function renderActivity(
  root: HTMLElement,
  item: QueueItem,
  snapshot: MonitoringSnapshot,
  showError: (message: string) => void,
) {
  const rows = activityRows(item, snapshot);
  if (!rows.length) return;
  const now = Date.now() / 1000;
  const named = new Set(rows.map((row) => row.agent).filter(Boolean)).size > 1;
  const list = document.createElement("ol");
  list.className = "queue-activity";
  list.dataset.queueActivity = "";
  list.setAttribute("aria-label", "Activity");
  for (const row of rows) {
    const entry = document.createElement("li");
    entry.dataset.activity = row.kind;
    const head = document.createElement("div");
    head.className = "activity-head";
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "activity-toggle";
    toggle.setAttribute("aria-expanded", "false");
    toggle.innerHTML = icon(row.icon);
    const parts: [string, string][] = [["activity-verb", row.verb]];
    if (named && row.agent) parts.push(["activity-agent", row.agent]);
    parts.push(["activity-value", row.value]);
    if (row.at) parts.push(["activity-time", compactDuration(now - row.at)]);
    for (const [className, text] of parts)
      toggle.append(
        Object.assign(document.createElement("span"), {
          className,
          textContent: text,
        }),
      );
    head.append(toggle);
    if (row.open) {
      const open = document.createElement("button");
      open.type = "button";
      open.className = "activity-open";
      open.title = "Open on GitHub";
      open.setAttribute(
        "aria-label",
        `Open ${row.verb.toLowerCase()} on GitHub`,
      );
      open.innerHTML = icon(paths.open);
      open.onclick = async () => {
        open.disabled = true;
        await openDestination(item.id, null, showError);
        open.disabled = false;
      };
      head.append(open);
    }
    const facts = document.createElement("div");
    facts.className = "activity-facts";
    facts.hidden = true;
    keyValues(facts, row.facts);
    toggle.onclick = () => {
      const open = facts.hidden;
      facts.hidden = !open;
      toggle.setAttribute("aria-expanded", String(open));
    };
    entry.append(head, facts);
    list.append(entry);
  }
  root.append(list);
}
