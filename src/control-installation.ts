import { invoke } from "@tauri-apps/api/core";

interface Installation {
  state: "absent" | "installed" | "incomplete";
  revision: string | null;
  mcp_registered: boolean;
  skill_detected: boolean;
  skill_command: string;
}

const SKILL_COMMAND =
  "npx skills add jdylanmc/pr-sniper --skill pr-sniper -g -a github-copilot";
const COPY_ICON =
  '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/></svg>';

export function mountControlInstallation(container: HTMLElement) {
  container.innerHTML = `<fieldset aria-label="Copilot CLI"><legend>Copilot CLI</legend>
    <div class="setting-row"><span>MCP</span><output aria-label="MCP status">Loading</output></div>
    <div class="resource-actions"><button type="button" data-install-mcp disabled>Install</button><button type="button" data-remove-mcp aria-label="Remove PR Sniper MCP server" disabled><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 10v7M14 10v7"/></svg></button></div>
    <div class="setting-row"><span>/pr-sniper</span><output aria-label="Skill status">Loading</output></div>
    <div class="setting-row skill-command"><code aria-label="Skill install command">${SKILL_COMMAND}</code><button type="button" data-copy-skill aria-label="Copy skill install command" title="Copy">${COPY_ICON}</button></div>
    <p role="alert" hidden></p></fieldset>`;
  const [status, skill] = container.querySelectorAll("output");
  const command = container.querySelector("code")!;
  const install =
    container.querySelector<HTMLButtonElement>("[data-install-mcp]")!;
  const remove =
    container.querySelector<HTMLButtonElement>("[data-remove-mcp]")!;
  const copy = container.querySelector<HTMLButtonElement>("[data-copy-skill]")!;
  const alert = container.querySelector<HTMLElement>("[role=alert]")!;
  let current: Installation | undefined;
  let pending = false;
  const render = () => {
    status.textContent =
      current?.state === "installed"
        ? "Installed"
        : current?.state === "incomplete"
          ? "Incomplete"
          : current
            ? "Not installed"
            : "Unavailable";
    install.textContent =
      current?.state === "installed"
        ? "Update"
        : current?.state === "incomplete"
          ? "Repair"
          : !current && !alert.hidden
            ? "Retry"
            : "Install";
    skill.textContent = current
      ? current.skill_detected
        ? "Detected"
        : "Not detected"
      : "Unavailable";
    command.textContent = current?.skill_command ?? SKILL_COMMAND;
    install.disabled = pending || (!current && Boolean(alert.hidden));
    remove.disabled = pending || !current?.revision;
  };
  const perform = async (
    command: string,
    args?: Record<string, unknown>,
    opener?: HTMLElement,
  ) => {
    if (pending) return;
    let focusMoved = false;
    const onInteraction = () => {
      focusMoved = true;
    };
    document.addEventListener("pointerdown", onInteraction, true);
    document.addEventListener("keydown", onInteraction, true);
    pending = true;
    if (command !== "control_installation") alert.hidden = true;
    render();
    try {
      current = await invoke<Installation>(command, args);
    } catch (error) {
      const code =
        typeof error === "string"
          ? error
          : error && typeof error === "object" && "code" in error
            ? error.code
            : "";
      if (command !== "control_installation" || alert.hidden)
        alert.textContent =
          code === "installation_conflict"
            ? "Conflict"
            : code === "installation_linked_unsupported"
              ? "Linked folder"
              : code === "conflict"
                ? "Changed"
                : code === "app_not_installed"
                  ? "App not installed"
                  : "Unavailable";
      alert.hidden = false;
      current = undefined;
      if (command !== "control_installation") {
        try {
          current = await invoke<Installation>("control_installation");
        } catch {
          // Preserve the mutation failure; Retry only reads status.
        }
      }
    } finally {
      document.removeEventListener("pointerdown", onInteraction, true);
      document.removeEventListener("keydown", onInteraction, true);
      pending = false;
      if (container.isConnected) {
        render();
        if (
          opener &&
          !focusMoved &&
          (document.activeElement === document.body ||
            document.activeElement === opener)
        )
          (opener.matches(":disabled") ? install : opener).focus({
            preventScroll: true,
          });
      }
    }
  };
  install.onclick = () =>
    void perform(
      current ? "control_install" : "control_installation",
      current
        ? {
            host: "copilot_cli",
            expected: current?.revision ?? null,
          }
        : undefined,
      install,
    );
  remove.onclick = () =>
    void perform(
      "control_uninstall",
      {
        host: "copilot_cli",
        expected: current?.revision,
      },
      remove,
    );
  copy.onclick = () => {
    void navigator.clipboard
      ?.writeText(command.textContent ?? "")
      .catch(() => undefined);
  };
  void perform("control_installation");
}
