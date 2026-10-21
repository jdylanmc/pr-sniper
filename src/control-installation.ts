import { invoke } from "@tauri-apps/api/core";

interface Installation {
  state: "absent" | "installed" | "incomplete";
  revision: string | null;
  version: string | null;
}

export function mountControlInstallation(container: HTMLElement) {
  container.innerHTML = `<fieldset aria-label="Copilot CLI"><legend>Copilot CLI</legend>
    <div class="setting-row"><span>MCP · /pr-sniper</span><output aria-label="Skill status">Loading</output></div>
    <div class="resource-actions"><button type="button" data-install-skill disabled>Install</button><button type="button" data-remove-skill aria-label="Remove PR Sniper skill" disabled><svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 10v7M14 10v7"/></svg></button></div>
    <p role="alert" hidden></p></fieldset>`;
  const status = container.querySelector("output")!;
  const install = container.querySelector<HTMLButtonElement>(
    "[data-install-skill]",
  )!;
  const remove = container.querySelector<HTMLButtonElement>(
    "[data-remove-skill]",
  )!;
  const alert = container.querySelector<HTMLElement>("[role=alert]")!;
  let current: Installation | undefined;
  let pending = false;
  const render = () => {
    status.textContent =
      current?.state === "installed"
        ? current.version
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
            : code === "conflict"
              ? "Changed"
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
  void perform("control_installation");
}
