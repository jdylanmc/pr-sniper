// Packet exits and host progress: icons and short labels, no prose.
export type ExitName =
  | "leave_comments"
  | "ready_for_human"
  | "approve_pull_request"
  | "approve_and_merge"
  | "panic_exit"
  | "submit_report";

export type Progress = "approving" | "merging" | "publishing";

const paths = {
  comment: "M4 5h16v11H9l-5 4V5Z",
  hand: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm-4 9 3 3 5-6",
  approve: "M5 12.5 10 17.5 19 7",
  merge: "M6 3v12m0 0a3 3 0 1 0 .01 0M18 9a3 3 0 1 0 .01 0M18 12c0 3-3 3-12 3",
  panic: "M12 3 2 20h20L12 3Zm0 6v5m0 3v.5",
  report: "M7 3h7l5 5v13H7V3Zm7 0v5h5M10 13h6m-6 4h6",
};

export const exitPresentation: Record<ExitName, [string, string]> = {
  leave_comments: ["Comments", paths.comment],
  ready_for_human: ["Ready", paths.hand],
  approve_pull_request: ["Approve", paths.approve],
  approve_and_merge: ["Approve + merge", paths.merge],
  panic_exit: ["Panic", paths.panic],
  submit_report: ["Report", paths.report],
};

export const progressPresentation: Record<Progress, [string, string]> = {
  approving: ["Approving", paths.approve],
  merging: ["Merging", paths.merge],
  publishing: ["Publishing", paths.comment],
};

export function svg(path: string) {
  return `<svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
}

// Spinner-free progress chip: icon + label.
export function progressChip(progress: Progress): HTMLElement {
  const [label, path] = progressPresentation[progress];
  const chip = document.createElement("span");
  chip.className = "queue-progress";
  chip.dataset.queueProgress = progress;
  chip.innerHTML = svg(path);
  chip.append(label);
  return chip;
}
