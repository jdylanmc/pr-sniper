import { invoke } from "@tauri-apps/api/core";
import { actualIntelligence } from "./copilot";
import type { QueueItem } from "./queue";
import type { ReviewSelection } from "./resources";
import { renderConfiguration } from "./work-presentation";

interface Operation {
  state: string;
  attempt_count: number;
  retry_deadline: number;
}

interface ConversationContext {
  job: QueueItem["job"];
  selection: ReviewSelection;
}
interface ConversationThread {
  id: string;
  comments: { id: string; body: string; author_login: string | null }[];
}
export interface MentionRouting {
  work_id: string;
  binding: { repository_name: string; number: number; account_id: string };
  comment: { id: string; body: string };
  blocked: string | null;
  follow_up_id: string | null;
}

export interface FollowUpCandidate {
  planned_selection?: ReviewSelection | null;
  blocked: string | null;
  automatic_start: boolean;
  automatic_publication: boolean;
  human_gate: boolean;
  run: {
    reply_ordinal?: number | null;
    enqueue_order?: number | null;
    id: string;
    phase: string;
    trigger_id: string;
    cancelled: boolean;
    uncertain: boolean;
    receipt: string | null;
    error: string | null;
    analysis: Operation | null;
    publication: Operation | null;
    context?: ConversationContext;
    analysis_history?: { context: ConversationContext; operation: Operation }[];
    discussion?: { id: string; body: string; author_login: string | null }[];
    target?:
      | {
          kind: "owned";
          publication_id: string;
          review: ConversationContext;
          thread: ConversationThread;
        }
      | {
          kind: "mention";
          comment: { id: string; body: string; author_login: string | null };
        }
      | {
          kind: "thread" | "retained";
          thread: ConversationThread;
        };
    review?: ConversationContext;
    thread?: ConversationThread;
    result: {
      output: {
        decision: "reply" | "quiet" | "human_input_required";
        body: string;
        reason: string;
        evidence: { path: string; side: string; line: number; quote: string }[];
      };
      session_id: string;
      model: string;
      intelligence?: import("./policy").AgentIntelligence | null;
    } | null;
  };
}

export function renderFollowUps(
  root: HTMLElement,
  showError: (message: string) => void,
  refresh: () => Promise<void>,
) {
  const expanded = new Set<string>();
  const pending = new Set<string>();
  let signature = "";
  async function act(
    candidate: FollowUpCandidate,
    publish: boolean,
    cancel: boolean,
  ) {
    const id = candidate.run.id;
    if (pending.has(id)) return;
    pending.add(id);
    try {
      if (cancel) await invoke("cancel_follow_up", { id });
      else
        await invoke("start_follow_up", {
          id,
          publish,
        });
    } catch (error) {
      showError(
        typeof error === "string"
          ? error
          : "Thread follow-up action failed. Check its persisted state before retrying.",
      );
    } finally {
      pending.delete(id);
      signature = "";
      await refresh();
    }
  }
  return (
    candidates: FollowUpCandidate[],
    all = candidates,
    mentions: MentionRouting[] = [],
    configuration = true,
  ) => {
    const working = all.some(({ run }) => run.publication?.state === "running");
    const next = JSON.stringify([candidates, working, mentions, configuration]);
    if (next === signature) return;
    signature = next;
    root.replaceChildren();
    for (const mention of mentions.filter((m) => m.blocked)) {
      const row = document.createElement("article");
      row.textContent = `${mention.binding.repository_name} #${mention.binding.number}: ${mention.blocked} Mention ${mention.comment.id}: ${mention.comment.body}`;
      root.append(row);
    }
    if (!candidates.length && !mentions.some((m) => m.blocked)) {
      root.textContent =
        "No new eligible other-user comments on admitted open pull requests.";
      return;
    }
    for (const candidate of candidates) {
      const run = candidate.run;
      const execution = run.context ?? run.review;
      if (!execution) {
        const error = document.createElement("p");
        error.textContent =
          "Conversation execution context unavailable; no action permitted.";
        root.append(error);
        continue;
      }
      const job = execution.job;
      const thread =
        run.target && "thread" in run.target ? run.target.thread : run.thread;
      const comments =
        run.target?.kind === "mention"
          ? run.discussion?.length
            ? run.discussion
            : [run.target.comment]
          : (thread?.comments ?? []);
      const row = document.createElement("article");
      row.className = "review-run";
      const heading = document.createElement("h3");
      heading.textContent = `${job.repository_name} #${job.number} / ${execution.selection.agent.name}`;
      const identity = document.createElement("p");
      identity.className = "hint";
      identity.textContent = `GitHub: ${job.account_login} (${job.account_id}); analysis head ${job.head_sha}. ${run.target?.kind === "mention" ? "Primary conversation" : `Thread ${thread?.id ?? "unavailable"}`}; external comment ${run.trigger_id}. Copilot account: ${execution.selection.agent.ai_account?.account_id ?? "unavailable"}; model: ${execution.selection.agent.model}.`;
      if (run.target?.kind === "owned")
        identity.append(
          ` Original root head ${run.target.review.job.head_sha}; publication ${run.target.publication_id}.`,
        );
      const state = document.createElement("p");
      state.textContent = `Follow-up: ${run.phase.replaceAll("_", " ")}. Execution is automatic when eligible; Reply Comment: ${candidate.automatic_publication ? "permitted after revalidation" : "off; analysis continues and qualifying responses remain local"}. Initial Publish Comment is independent.`;
      row.append(heading, identity, state);
      if (configuration && (run.analysis?.attempt_count || run.result))
        renderConfiguration(
          row,
          "Captured conversation configuration",
          execution.selection,
        );
      if (run.analysis_history?.length) {
        const history = document.createElement("details");
        const title = document.createElement("summary");
        title.textContent = "Prior executed analysis attempts";
        history.append(title);
        for (const attempt of run.analysis_history) {
          const state = document.createElement("p");
          state.textContent = `Attempt ${attempt.operation.attempt_count}: ${attempt.operation.state}. ${attempt.context.job.repository_name} #${attempt.context.job.number}, head ${attempt.context.job.head_sha}.`;
          history.append(state);
          renderConfiguration(
            history,
            "Actual prior analysis configuration",
            attempt.context.selection,
          );
        }
        row.append(history);
      }
      if (
        (configuration && !run.analysis?.attempt_count && !run.result) ||
        (run.analysis &&
          run.analysis.attempt_count > 0 &&
          ["queued", "interrupted"].includes(run.analysis.state))
      )
        renderConfiguration(
          row,
          "Planned conversation configuration (revalidated at start)",
          candidate.planned_selection,
          candidate.blocked ?? undefined,
        );
      if (run.reply_ordinal != null) {
        const order = document.createElement("p");
        order.textContent = `Reply work ${run.reply_ordinal}; queue order ${run.enqueue_order ?? "not recorded"}. Retry attempts are separate.`;
        row.append(order);
      }
      for (const [label, operation] of [
        ["Analysis", run.analysis],
        ["Publication", run.publication],
      ] as const) {
        if (!operation) continue;
        const detail = document.createElement("p");
        detail.className = "hint";
        detail.textContent = `${label}: ${operation.state}; attempt ${operation.attempt_count}; deadline ${label === "Analysis" && operation.attempt_count === 0 ? "starts at first execution" : new Date(operation.retry_deadline * 1000).toLocaleString()}.`;
        row.append(detail);
      }
      if (candidate.human_gate) {
        const warning = document.createElement("p");
        warning.textContent =
          "A prior comment needs human judgment. Retry this follow-up only after the human decision.";
        row.append(warning);
      }
      if (candidate.blocked || run.error) {
        const error = document.createElement("p");
        error.className = "review-failure";
        error.textContent = [candidate.blocked, run.error]
          .filter(Boolean)
          .join(" ");
        row.append(error);
      }
      if (run.uncertain || run.receipt) {
        const receipt = document.createElement("p");
        receipt.textContent = run.receipt
          ? `Confirmed GitHub reply ${run.receipt}. This is not approval; cancellation cannot remove it.`
          : "Reply outcome unresolved. Reconciliation checks the original reply and never blindly posts a replacement.";
        row.append(receipt);
      }
      const conversation = document.createElement("details");
      conversation.open = expanded.has(run.id);
      conversation.ontoggle = () => {
        if (conversation.open) expanded.add(run.id);
        else expanded.delete(run.id);
      };
      const title = document.createElement("summary");
      title.textContent = `Conversation (${comments.length} comments)`;
      const list = document.createElement("ol");
      for (const comment of comments) {
        const item = document.createElement("li");
        item.textContent = `${comment.author_login ?? "Unavailable author"}: ${comment.body}`;
        list.append(item);
      }
      conversation.append(title, list);
      row.append(conversation);
      if (run.context && run.target) {
        const captured = document.createElement("details");
        const label = document.createElement("summary");
        label.textContent = "Original target and captured analysis context";
        const body = document.createElement("pre");
        body.textContent = JSON.stringify(
          { target: run.target, context: run.context },
          null,
          2,
        );
        captured.append(label, body);
        row.append(captured);
      }
      if (run.result) {
        const session = document.createElement("p");
        session.className = "hint";
        session.textContent = `Session ${run.result.session_id}; model ${run.result.model}. ${actualIntelligence(run.result.intelligence)}`;
        row.append(session);
        const decision = document.createElement("p");
        decision.textContent =
          run.result.output.decision === "quiet"
            ? `No reply needed. ${run.result.output.reason}`
            : run.result.output.decision === "human_input_required"
              ? `Human input required. No automated reply. ${run.result.output.reason}`
              : `Draft reply: ${run.result.output.body}`;
        row.append(decision);
        for (const evidence of run.result.output.evidence) {
          const source = document.createElement("p");
          source.textContent = `${evidence.path}, ${evidence.side} line ${evidence.line}: ${evidence.quote}`;
          row.append(source);
        }
      }
      const running =
        run.analysis?.state === "running" ||
        run.publication?.state === "running";
      const publishing = run.result?.output.decision === "reply";
      const terminal =
        run.publication?.state === "completed" || (run.result && !publishing);
      const queued =
        !run.result &&
        !!run.analysis &&
        ["queued", "interrupted"].includes(run.analysis.state);
      if ((running || queued) && !run.cancelled && !run.receipt) {
        const button = document.createElement("button");
        button.textContent = queued
          ? "Cancel queued follow-up"
          : "Cancel follow-up";
        button.disabled = pending.has(run.id);
        button.onclick = () => {
          button.disabled = true;
          void act(candidate, false, true);
        };
        row.append(button);
      } else if (
        !terminal &&
        (!candidate.blocked || run.publication) &&
        (publishing || run.analysis || run.cancelled || candidate.human_gate) &&
        (!publishing ||
          candidate.automatic_publication ||
          run.uncertain ||
          run.receipt)
      ) {
        const button = document.createElement("button");
        button.textContent = publishing
          ? run.publication
            ? "Reconcile / retry reply"
            : "Publish reply"
          : candidate.human_gate
            ? "Retry after human decision"
            : "Retry follow-up";
        const update = () => {
          button.disabled = pending.has(run.id) || (!!publishing && working);
        };
        update();
        button.onclick = () => {
          button.disabled = true;
          void act(candidate, !!publishing, false);
        };
        row.append(button);
      }
      root.append(row);
    }
  };
}
