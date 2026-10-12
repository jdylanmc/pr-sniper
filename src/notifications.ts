import { invoke } from "@tauri-apps/api/core";

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
  targets: { id: string; label: string }[];
}

const phase: Record<Notice["phase"], string> = {
  queued: "Queued",
  submitting: "Submitting",
  accepted_unconfirmed: "Accepted",
  outcome_unknown: "Unknown",
  permission_denied: "Permission denied",
  failed: "Failed",
  not_sent: "Not sent",
};

function permissionText(state: Snapshot) {
  const permission = state.permission;
  const os = state.platform === "windows" ? "Windows" : "macOS";
  const channel = (value: boolean | null) =>
    value === null ? "unknown" : value ? "on" : "off";
  return `${state.enabled ? "On" : "Off"}; ${
    permission
      ? `${os} ${permission.authorization.replaceAll("_", " ")}; banners ${channel(permission.alerts_enabled)}; center ${channel(permission.center_enabled)}`
      : `${os} unavailable`
  }`;
}

export function mountNotificationSettings(
  root: HTMLElement,
  view: {
    target: string;
    pending?: boolean;
    actionError?: string | null;
    refresh?: () => Promise<void>;
  },
) {
  root.innerHTML = `<fieldset aria-label="Notifications"><legend>Notifications</legend>
    <label class="setting-row"><span>Notifications</span><input id="notification-enabled" type="checkbox" role="switch" aria-describedby="notification-permission" disabled /></label>
    <p id="notification-permission" role="status">Loading</p>
    <p id="notification-error" role="alert" hidden></p>
    <label>Test destination<select id="notification-target"><option value="">Settings</option></select></label>
    <button id="notification-test" type="button" disabled>Send test notification</button></fieldset>`;
  const enabled = root.querySelector<HTMLInputElement>(
    "#notification-enabled",
  )!;
  const status = root.querySelector<HTMLElement>("#notification-permission")!;
  const error = root.querySelector<HTMLElement>("#notification-error")!;
  const target = root.querySelector<HTMLSelectElement>("#notification-target")!;
  const test = root.querySelector<HTMLButtonElement>("#notification-test")!;
  let reading = false;
  let revision = 0;
  let selected = view.target;
  let targetsSignature = "";
  let state: Snapshot | undefined;
  target.onchange = () => {
    selected = target.value;
    view.target = selected;
    updateControls();
  };

  function updateControls() {
    enabled.disabled = !!view.pending || !state;
    test.disabled =
      !!view.pending ||
      !state?.enabled ||
      (!!selected && !state.targets.some((t) => t.id === selected));
    target.disabled = !!view.pending || !state;
  }

  async function refresh() {
    if (view.pending || reading || !root.isConnected) return;
    reading = true;
    const captured = revision;
    try {
      const next = await invoke<Snapshot>("notification_snapshot");
      if (!root.isConnected || view.pending || captured !== revision) return;
      state = next;
      enabled.checked = next.enabled;
      status.textContent = permissionText(next);
      error.textContent = [view.actionError, next.error]
        .filter(Boolean)
        .join("\n");
      error.hidden = !error.textContent;
      const targets = JSON.stringify(next.targets);
      if (targets !== targetsSignature) {
        targetsSignature = targets;
        target.replaceChildren(new Option("Settings", ""));
        for (const item of next.targets)
          target.append(new Option(item.label, item.id));
        if (selected && !next.targets.some((t) => t.id === selected)) {
          target.append(new Option("Unavailable", selected));
        }
        target.value = selected;
      }
    } catch {
      if (!root.isConnected || captured !== revision) return;
      state = undefined;
      status.textContent = "Unavailable";
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

  async function act(command: string, args: Record<string, unknown>) {
    if (view.pending) return;
    view.pending = true;
    view.actionError = null;
    error.hidden = true;
    revision++;
    updateControls();
    status.textContent =
      command === "set_notifications_enabled" && args.enabled
        ? "Checking permission"
        : "Updating";
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
    void act("set_notifications_enabled", { enabled: enabled.checked });
  test.onclick = () =>
    void act("test_notification", { itemId: selected || null });
  view.refresh = refresh;
  updateControls();
  if (view.pending) status.textContent = "Pending";
  void refresh();
  const timer = window.setInterval(() => {
    if (!root.isConnected) window.clearInterval(timer);
    else void refresh();
  }, 5000);
}

export function renderNotificationHistory(
  root: HTMLElement,
  showError: (message: string) => void,
) {
  let signature = "";
  let expanded = false;
  let loading = false;
  return async () => {
    if (loading || !root.isConnected) return;
    loading = true;
    try {
      const state = await invoke<Snapshot>("notification_snapshot");
      if (!root.isConnected) return;
      const next = JSON.stringify(state);
      if (next === signature) return;
      signature = next;
      root.replaceChildren();
      root.removeAttribute("role");
      const status = document.createElement("p");
      status.textContent = permissionText(state);
      root.append(status);
      if (state.error) {
        const error = document.createElement("p");
        error.setAttribute("role", "alert");
        error.textContent = state.error;
        root.append(error);
      }
      const details = document.createElement("details");
      details.open = expanded;
      details.ontoggle = () => {
        expanded = details.open;
      };
      const summary = document.createElement("summary");
      summary.textContent = `Notification history (${state.notices.length})`;
      details.append(summary);
      for (const notice of state.notices) {
        const row = document.createElement("article");
        const title = document.createElement("h3");
        title.textContent = notice.title;
        const info = document.createElement("p");
        info.textContent = `${new Date(notice.created_at * 1000).toLocaleString()}: ${phase[notice.phase]}`;
        row.append(title, info);
        for (const message of [notice.error, notice.navigation_error].filter(
          Boolean,
        )) {
          const error = document.createElement("p");
          error.textContent = message;
          row.append(error);
        }
        if (notice.opened_at !== null) {
          const opened = document.createElement("p");
          opened.textContent = `Opened ${new Date(notice.opened_at * 1000).toLocaleString()}`;
          row.append(opened);
        }
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = "Open saved destination";
        button.onclick = async () => {
          button.disabled = true;
          try {
            await invoke("open_notification", { id: notice.id });
          } catch (cause) {
            showError(typeof cause === "string" ? cause : "Open failed");
          } finally {
            button.disabled = false;
            signature = "";
            root.setAttribute("role", "alert");
          }
        };
        row.append(button);
        details.append(row);
      }
      root.append(details);
    } catch {
      signature = "";
      root.textContent = "Unavailable";
    } finally {
      loading = false;
    }
  };
}
