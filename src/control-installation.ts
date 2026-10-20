import { invoke } from "@tauri-apps/api/core";

interface Installation {
  state: "absent" | "installed";
  revision: string | null;
  version: string | null;
}

export function mountControlInstallation(container: HTMLElement) {
  container.innerHTML = `<fieldset aria-label="Copilot CLI"><legend>Copilot CLI</legend>
    <div class="setting-row"><span>/pr-sniper</span><output aria-label="Skill status">Loading</output></div>
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
        : current
          ? "Not installed"
          : "Unavailable";
    install.textContent = current?.state === "installed" ? "Update" : "Install";
    install.disabled = pending || !current;
    remove.disabled = pending || current?.state !== "installed";
  };
  const perform = async (command: string, args?: Record<string, unknown>) => {
    if (pending) return;
    pending = true;
    alert.hidden = true;
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
      alert.textContent =
        code === "installation_conflict"
          ? "Conflict"
          : code === "conflict"
            ? "Changed"
            : "Unavailable";
      alert.hidden = false;
      current = undefined;
    } finally {
      pending = false;
      if (container.isConnected) render();
    }
  };
  install.onclick = () =>
    void perform("control_install", {
      host: "copilot_cli",
      expected: current?.revision ?? null,
    });
  remove.onclick = () =>
    void perform("control_uninstall", {
      host: "copilot_cli",
      expected: current?.revision,
    });
  void perform("control_installation");
}
