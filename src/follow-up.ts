import { invoke } from "@tauri-apps/api/core";

interface Operation {
  state: string;
  attempt_count: number;
  retry_deadline: number;
}

export interface FollowUpCandidate {
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
    review: {
      job: {
        repository_name: string;
        number: number;
        account_login: string;
        account_id: string;
        head_sha: string;
      };
      selection: {
        agent: {
          name: string;
          model: string;
          ai_account?: { account_id: string } | null;
        };
      };
    };
    thread: {
      id: string;
      comments: { id: string; body: string; author_login: string | null }[];
    };
    result: {
      output: {
        decision: "reply" | "quiet" | "human_input_required";
        body: string;
        reason: string;
        evidence: { path: string; side: string; line: number; quote: string }[];
      };
      session_id: string;
      model: string;
    } | null;
  };
}

export function renderFollowUps(
  root: HTMLElement,
  showError: (message: string) => void,
  refresh: () => Promise<void>,
) {
  const consent = new Set<string>();
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
      else await invoke("start_follow_up", { id, publish });
    } catch (error) {
      showError(
        typeof error === "string"
          ? error
          : "Thread follow-up action failed. Check its persisted state before retrying.",
      );
    } finally {
      pending.delete(id);
      consent.delete(id);
      signature = "";
      await refresh();
    }
  }
  return (candidates: FollowUpCandidate[], all = candidates) => {
    const working = all.some(
      ({ run }) =>
        run.analysis?.state === "running" ||
        run.publication?.state === "running",
    );
    const next = JSON.stringify([candidates, working]);
    if (next === signature) return;
    signature = next;
    root.replaceChildren();
    if (!candidates.length) {
      root.textContent =
        "No new external comments in verified PR Sniper-owned threads.";
      return;
    }
    for (const candidate of candidates) {
      const run = candidate.run;
      const job = run.review.job;
      const row = document.createElement("article");
      row.className = "review-run";
      const heading = document.createElement("h3");
      heading.textContent = `${job.repository_name} #${job.number} / ${run.review.selection.agent.name}`;
      const identity = document.createElement("p");
      identity.className = "hint";
      identity.textContent = `GitHub: ${job.account_login} (${job.account_id}); head ${job.head_sha}. Thread ${run.thread.id}; external comment ${run.trigger_id}. Copilot account: ${run.review.selection.agent.ai_account?.account_id ?? "unavailable"}; model: ${run.review.selection.agent.model}.`;
      const state = document.createElement("p");
      state.textContent = `Follow-up: ${run.phase.replaceAll("_", " ")}. Start: ${candidate.automatic_start ? "automatic" : "manual"}; reply publication: ${candidate.automatic_publication ? "automatic" : "confirmation required"}.`;
      row.append(heading, identity, state);
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
        detail.textContent = `${label}: ${operation.state}; attempt ${operation.attempt_count}; deadline ${new Date(operation.retry_deadline * 1000).toLocaleString()}.`;
        row.append(detail);
      }
      if (candidate.human_gate) {
        const warning = document.createElement("p");
        warning.textContent =
          "A prior comment needs human judgment. Automatic follow-ups are paused; start explicitly only after the human decision.";
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
      title.textContent = `Conversation (${run.thread.comments.length} comments)`;
      const list = document.createElement("ol");
      for (const comment of run.thread.comments) {
        const item = document.createElement("li");
        item.textContent = `${comment.author_login ?? "Unavailable author"}: ${comment.body}`;
        list.append(item);
      }
      conversation.append(title, list);
      row.append(conversation);
      if (run.result) {
        const session = document.createElement("p");
        session.className = "hint";
        session.textContent = `Session ${run.result.session_id}; model ${run.result.model}.`;
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
      if (running && !run.cancelled && !run.receipt) {
        const button = document.createElement("button");
        button.textContent = "Cancel follow-up";
        button.disabled = pending.has(run.id);
        button.onclick = () => {
          button.disabled = true;
          void act(candidate, false, true);
        };
        row.append(button);
      } else if (!terminal && (!candidate.blocked || run.publication)) {
        const button = document.createElement("button");
        button.textContent = publishing
          ? run.publication
            ? "Reconcile / retry reply"
            : "Publish reply"
          : run.analysis
            ? "Retry follow-up"
            : "Start follow-up";
        const update = () => {
          button.disabled =
            pending.has(run.id) ||
            working ||
            (!!publishing && !consent.has(run.id));
        };
        if (publishing) {
          const label = document.createElement("label");
          const checkbox = document.createElement("input");
          checkbox.type = "checkbox";
          checkbox.checked = consent.has(run.id);
          label.append(
            checkbox,
            `Publish or reconcile this thread/comment/revision as ${job.account_login} (${job.account_id}), not approval.`,
          );
          checkbox.onchange = () => {
            if (checkbox.checked) consent.add(run.id);
            else consent.delete(run.id);
            update();
          };
          row.append(label);
        }
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
