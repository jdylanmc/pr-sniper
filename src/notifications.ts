import { invoke } from "@tauri-apps/api/core";
import { setText } from "./stable-dom";

interface Notice {
  id: string;
  title: string;
  body: string;
  phase:
    | "queued"
    | "submitting"
    | "accepted_unconfirmed"
    | "outcome_unknown"
    | "permission_denied"
    | "failed"
    | "not_sent";
  created_at: number;
  error: string | null;
  opened_at: number | null;
  navigation_error: string | null;
}
interface Snapshot {
  platform?: string;
  enabled: boolean;
  permission: {
    authorization: string;
    alerts_enabled: boolean | null;
    center_enabled: boolean | null;
  } | null;
  error: string | null;
  notices: Notice[];
  categories: { category: Category; enabled: boolean }[];
}

type Category =
  | "confirmation"
  | "human_input"
  | "ready"
  | "failure"
  | "approved"
  | "merged"
  | "final_review_cleared";

const categoryLabel: Record<Category, string> = {
  confirmation: "Confirmation",
  human_input: "Needs input",
  ready: "Ready",
  failure: "Failures",
  approved: "Approved",
  merged: "Merged",
  final_review_cleared: "Final review cleared",
};

// Healthy notifications show nothing beyond the switch; a problem shows one
// short label. Unknown channels are never treated as muted.
function permissionAlert(state: Snapshot): string | null {
  const os = state.platform === "windows" ? "Windows" : "macOS";
  const permission = state.permission;
  if (!permission) return state.enabled ? `${os} unavailable` : null;
  const { authorization } = permission;
  if (authorization === "authorized_aggregate") return null;
  if (authorization === "authorized" || authorization === "provisional")
    return state.enabled &&
      permission.alerts_enabled === false &&
      permission.center_enabled === false
      ? `Muted in ${os}`
      : null;
  if (["not_determined", "not_registered", "unknown"].includes(authorization))
    return state.enabled ? `Blocked by ${os}` : null;
  return `Blocked by ${os}`;
}

const alertIcon =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M12 8v5m0 3h.01M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18"/></svg>';

export function mountNotificationSettings(
  root: HTMLElement,
  view: {
    pending?: boolean;
    actionError?: string | null;
    refresh?: () => Promise<void>;
    snapshot?: Snapshot;
  },
) {
  root.innerHTML = `<fieldset aria-label="Notifications">
    <label class="setting-row"><span>Notifications</span><input id="notification-enabled" type="checkbox" role="switch" aria-describedby="notification-permission" disabled /></label>
    <p id="notification-permission" class="notification-alert" hidden>${alertIcon}<span data-notification-alert></span></p><span class="sr-only" role="status" data-notification-announce></span>
    <p id="notification-error" role="alert" hidden></p>
    <fieldset id="notification-types" class="notification-types" hidden><legend>Notification types</legend>${(
      Object.keys(categoryLabel) as Category[]
    )
      .map(
        (category) =>
          `<label class="setting-row"><span>${categoryLabel[category]}</span><input id="notification-type-${category}" data-notification-type="${category}" type="checkbox" role="switch" disabled /></label>`,
      )
      .join("")}</fieldset></fieldset>`;
  const enabled = root.querySelector<HTMLInputElement>(
    "#notification-enabled",
  )!;
  const status = root.querySelector<HTMLElement>("#notification-permission")!;
  const showAlert = (label: string | null) => {
    setText(status.querySelector("[data-notification-alert]")!, label ?? "");
    status.hidden = !label;
    // The live region stays rendered so a new problem is announced.
    setText(root.querySelector("[data-notification-announce]")!, label ?? "");
  };
  const error = root.querySelector<HTMLElement>("#notification-error")!;
  const types = root.querySelector<HTMLElement>("#notification-types")!;
  const toggles = [
    ...types.querySelectorAll<HTMLInputElement>("[data-notification-type]"),
  ];
  let reading = false;
  let revision = 0;
  let state: Snapshot | undefined;
  let opener: string | undefined;

  // Redraws remount this card; showing the last read keeps controls focusable by ID.
  function apply(next: Snapshot) {
    state = next;
    view.snapshot = next;
    enabled.checked = next.enabled;
    showAlert(permissionAlert(next));
    error.textContent = [view.actionError, next.error]
      .filter(Boolean)
      .join("\n");
    error.hidden = !error.textContent;
    for (const toggle of toggles)
      toggle.checked = next.categories.some(
        (c) => c.category === toggle.dataset.notificationType && c.enabled,
      );
    types.hidden = !next.enabled;
  }

  function updateControls() {
    enabled.disabled = !!view.pending || !state;
    for (const toggle of toggles)
      toggle.disabled = !!view.pending || !state?.enabled;
    if (!opener || view.pending) return;
    // Disabling the acting switch drops its focus; return it unless the user moved on.
    const control = root.querySelector<HTMLInputElement>(`#${opener}`);
    if (control?.disabled) return;
    opener = undefined;
    const active = document.activeElement;
    if (control && !control.hidden && (!active || active === document.body))
      control.focus();
  }

  async function refresh() {
    if (view.pending || reading || !root.isConnected) return;
    reading = true;
    const captured = revision;
    try {
      const next = await invoke<Snapshot>("notification_snapshot");
      if (!root.isConnected || view.pending || captured !== revision) return;
      apply(next);
    } catch {
      if (!root.isConnected || captured !== revision) return;
      state = undefined;
      view.snapshot = undefined;
      showAlert(null);
      error.textContent = [view.actionError, "Unavailable"]
        .filter(Boolean)
        .join("\n");
      error.hidden = false;
    } finally {
      reading = false;
      updateControls();
      if (captured !== revision && !view.pending && root.isConnected)
        void refresh();
    }
  }

  async function act(
    control: HTMLInputElement,
    command: string,
    args: Record<string, unknown>,
  ) {
    if (view.pending) return;
    opener = control.id;
    view.pending = true;
    view.actionError = null;
    error.hidden = true;
    revision++;
    updateControls();
    try {
      await invoke(command, args);
    } catch (cause) {
      view.actionError = typeof cause === "string" ? cause : "Failed";
    } finally {
      view.pending = false;
      await view.refresh?.();
    }
  }
  enabled.onchange = () =>
    void act(enabled, "set_notifications_enabled", {
      enabled: enabled.checked,
    });
  for (const toggle of toggles)
    toggle.onchange = () =>
      void act(toggle, "set_notification_category", {
        category: toggle.dataset.notificationType,
        enabled: toggle.checked,
      });
  view.refresh = refresh;
  if (view.snapshot && !view.pending) apply(view.snapshot);
  updateControls();
  void refresh();
  const timer = window.setInterval(() => {
    if (!root.isConnected) window.clearInterval(timer);
    else void refresh();
  }, 5000);
}
