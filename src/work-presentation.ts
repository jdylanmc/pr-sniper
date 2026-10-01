import type { MonitoringSnapshot } from "./monitoring";
import type { WorkKind } from "./panel";
import type { QueueItem } from "./queue";
import type { ReviewSelection } from "./resources";

export const purposes: Record<WorkKind, string> = {
  normal: "Normal pass",
  primary_final: "Primary final review",
  reply: "Targeted reply",
  mention: "Primary mention",
};

export interface WorkPresentation {
  agent: string;
  subject: string;
  reference: string;
  ordinal: string;
  state: string;
  job?: QueueItem["job"];
  selection?: ReviewSelection | null;
  captured: boolean;
  attempt: number | null;
  trigger: string;
}

export function workPresentation(
  snapshot: MonitoringSnapshot,
  kind: WorkKind,
  id: string,
): WorkPresentation | undefined {
  let job: QueueItem["job"] | undefined;
  let selection: ReviewSelection | null | undefined;
  let state = "waiting";
  let captured = false;
  let attempt: number | null = null;
  let trigger = "Not recorded";
  let ordinal = purposes[kind];
  if (kind === "normal") {
    const candidate = snapshot.reviews?.find((r) => r.key === id);
    if (!candidate) return;
    job = candidate.run?.job ?? candidate.job;
    captured = !!candidate.run?.operation.attempt_count;
    selection = captured
      ? candidate.run?.selection
      : candidate.planned_selection;
    attempt = candidate.run?.operation.attempt_count ?? 0;
    state = candidate.run?.operation.state ?? "waiting";
    if (candidate.job.waiting === "superseded") state = "superseded";
    trigger = job.work?.trigger.replaceAll("_", " ") ?? "Legacy admission";
    ordinal = job.work ? `Pass ${job.work.pass_ordinal}` : "Normal pass";
  } else if (kind === "primary_final") {
    const final = snapshot.items?.find(
      (i) => i.action_status?.final_review?.id === id,
    )?.action_status?.final_review;
    if (!final) return;
    job = final.execution.job;
    selection = final.execution.selection;
    attempt = final.execution.operation.attempt_count;
    captured = attempt > 0;
    state = final.execution.operation.state;
    trigger =
      "Current passes and concerns clear; separately permitted provider action";
  } else {
    const candidate = snapshot.follow_ups?.find(
      (f) =>
        f.run.id === id &&
        (f.run.target?.kind === "mention" ? "mention" : "reply") === kind,
    );
    if (!candidate) {
      const mention =
        kind === "mention" && snapshot.mentions?.find((m) => m.work_id === id);
      if (!mention) return;
      return {
        agent: "Primary unavailable",
        subject: `Mention ${mention.comment.id}`,
        reference: `${mention.binding.repository_name} #${mention.binding.number}`,
        ordinal,
        state: "blocked",
        captured: false,
        attempt: null,
        trigger: `External comment ${mention.comment.id}; ${mention.blocked ?? "Awaiting admission"}`,
      };
    }
    const run = candidate.run;
    const context = run.context ?? run.review;
    job = context?.job;
    captured = !!run.analysis?.attempt_count || !!run.result;
    selection = captured ? context?.selection : candidate.planned_selection;
    attempt = run.analysis?.attempt_count ?? 0;
    state = run.analysis?.state ?? run.phase;
    trigger = `External comment ${run.trigger_id}; ${kind === "mention" ? "acting-account mention" : "owned-thread reply"}`;
    ordinal =
      run.reply_ordinal == null
        ? purposes[kind]
        : `${kind === "mention" ? "Mention" : "Reply"} ${run.reply_ordinal}`;
  }
  return {
    agent: selection?.agent.name ?? "Agent configuration unavailable",
    subject: job?.title ?? "PR context unavailable",
    reference: job
      ? `${job.repository_name} #${job.number}`
      : "Repository unavailable",
    ordinal,
    state,
    job,
    selection,
    captured,
    attempt,
    trigger,
  };
}

export function renderConfiguration(
  root: HTMLElement,
  label: string,
  selection: ReviewSelection | null | undefined,
  unavailable = "No saved configuration is available.",
) {
  const details = document.createElement("details");
  details.className = "work-configuration";
  const summary = document.createElement("summary");
  summary.textContent = label;
  details.append(summary);
  const text = (title: string, value: unknown) => {
    const heading = document.createElement("h4");
    heading.textContent = title;
    const body = document.createElement("pre");
    body.textContent =
      typeof value === "string"
        ? value
        : (JSON.stringify(value, null, 2) ?? "Not captured");
    details.append(heading, body);
  };
  if (!selection) text("Unavailable", unavailable);
  else {
    text("Agent", selection.agent);
    text("Agent prompt", selection.agent.prompt);
    text("Repository policy", selection.policy);
    text("Review preset", selection.preset);
    if (selection.configuration) {
      text("Repository and assignments", selection.configuration.repository);
      text(
        "Captured assignment authority (not a current provider grant)",
        selection.configuration.authority,
      );
      if (!selection.configuration.doctrines.length)
        text("Doctrines", "None selected");
      for (const doctrine of selection.configuration.doctrines)
        text(`Doctrine: ${doctrine.title}`, doctrine.body);
    } else {
      text(
        "Legacy snapshot",
        "Full doctrine and assignment configuration was not captured. Today's settings are not historical evidence.",
      );
      if (selection.doctrine)
        text("Retained legacy doctrine text", selection.doctrine);
    }
  }
  root.append(details);
}

export function renderWorkContext(
  root: HTMLElement,
  snapshot: MonitoringSnapshot,
  kind: WorkKind,
  id: string,
) {
  const work = workPresentation(snapshot, kind, id);
  if (!work) return;
  const section = document.createElement("section");
  section.dataset.workContext = id;
  const title = document.createElement("h2");
  title.textContent = `${work.agent} / ${purposes[kind]}`;
  section.append(title);
  const fact = (text: string) => {
    const line = document.createElement("p");
    line.textContent = text;
    section.append(line);
  };
  const authority = work.selection?.configuration?.authority;
  fact(
    `${work.reference}. ${work.captured ? "Captured" : "Planned"} role: ${authority ? (authority.primary ? "Primary Agent" : "Assigned Agent (not primary)") : "Not recorded; not inferred from current Settings"}.`,
  );
  fact(`Purpose: ${purposes[kind]}. Work ID: ${id}. Trigger: ${work.trigger}.`);
  const identity = work.job?.work;
  fact(
    identity
      ? `Iteration ${identity.iteration} (${identity.iteration_id}); item ${identity.item_id}; head ${work.job!.head_sha}.`
      : `Canonical iteration not recorded. Head ${work.job?.head_sha ?? "unavailable"} is not an iteration identity.`,
  );
  const same = (job: QueueItem["job"]) =>
    !!work.job &&
    (
      [
        "provider",
        "account_id",
        "configuration_id",
        "repository_id",
        "pull_request_id",
      ] as const
    ).every((key) => job[key] === work.job![key]);
  const agent = work.selection?.agent.id;
  const passes = (snapshot.reviews ?? [])
    .filter(
      (r) =>
        same(r.job) &&
        (r.run?.selection.agent.id ?? r.planned_selection?.agent.id) === agent,
    )
    .map((r) => r.job.work?.pass_ordinal)
    .filter((n): n is number => n != null);
  const replies = (snapshot.follow_ups ?? [])
    .filter(({ run }) => {
      const context = run.context ?? run.review;
      return (
        context && same(context.job) && context.selection.agent.id === agent
      );
    })
    .map(({ run }) => run.reply_ordinal)
    .filter((n): n is number => n != null);
  fact(
    `Per-Agent PR work: normal pass ordinal ${agent && passes.length ? Math.max(...passes) : "not recorded"}; reply/mention ordinal ${agent && replies.length ? Math.max(...replies) : "not recorded"}. Retry attempt for this job: ${work.attempt ?? "not recorded"}. Final review is separate from normal passes.`,
  );
  root.append(section);
}
