// Typed outcome of reviewed work (Rust `review::outcome::Outcome`).
export type Outcome =
  | "approved_and_merged"
  | "approved"
  | "commented"
  | "ready_for_human"
  | "waiting"
  | "report"
  | "pending"
  | "failed";

const clock = "M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18";

// Filter chip order.
export const outcomes: Record<Outcome, { label: string; icon: string }> = {
  approved_and_merged: {
    label: "Approved & merged",
    icon: "M6 9v12M6 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm12 6a3 3 0 1 0 0 6 3 3 0 0 0 0-6ZM6 9c0 4 4 6 9 6",
  },
  approved: {
    label: "Approved",
    icon: "m8 12 3 3 5-6M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  },
  commented: { label: "Commented", icon: "M4 5h16v11H9l-5 4V5Z" },
  ready_for_human: {
    label: "Ready for human",
    icon: "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm-7 9a7 7 0 0 1 14 0",
  },
  waiting: { label: "Waiting", icon: clock },
  report: {
    label: "Report",
    icon: "M7 3h7l5 5v13H7V3Zm7 0v5h5M10 13h6M10 17h6",
  },
  pending: {
    label: "Pending",
    icon: "M7 3h10M7 21h10M8 3c0 6 8 6 8 9s-8 3-8 9M16 3c0 6-8 6-8 9s8 3 8 9",
  },
  failed: {
    label: "Failed",
    icon: "m9 9 6 6m0-6-6 6M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  },
};
