import { invoke } from "@tauri-apps/api/core";
import {
  modelSelectable,
  intelligenceError,
  type CopilotModel,
} from "./copilot";
import { doctrineTitles, effectivePolicy } from "./policy";
import { type SetupReview } from "./resources";
import { mountSettings, type SetupTarget, type GuidedReturn } from "./settings";
import "./genie.css";

const escape = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (c) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[c]!,
  );
const spark =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="m12 3 3 6 6 3-6 3-3 6-3-6-6-3 6-3Z"/></svg>';
const targets: SetupTarget[] = [
  "ai",
  "repository-account",
  "agents",
  "repositories",
];
const titles = [
  "Choose your AI provider",
  "Connect source control",
  "Create your reviewer",
  "Choose what to watch",
];
const guidance = [
  "Pick the provider that will power your reviewers, then connect an account. Repository access comes separately.",
  "Connect the identity that can access your repositories. Connecting it does not connect or select an AI account.",
  "Choose an AI account and a returned model. Add zero, one or many shared doctrines.",
  "Choose the acting account, assign an Agent and set independent permissions. Save authorizes all currently open and future matching pull requests.",
];
const failure = (cause: unknown) =>
  typeof cause === "string"
    ? cause
    : "Setup could not be read. Retry; saved resources are unchanged.";
type SetupMode = "welcome" | "genie" | "review";

export function mountGenie(
  root: HTMLElement,
  options: {
    edit: (
      target: SetupTarget,
      opener: HTMLElement,
      origin: GuidedReturn,
    ) => void;
    manual: (opener: HTMLElement) => void;
    complete: (paused: boolean) => void;
    title: (title: string) => void;
    state?: (state: SetupReview) => void;
  },
) {
  root.classList.add("genie-page");
  let mode: SetupMode = "welcome";
  let state: SetupReview | undefined;
  let active = false;
  let request = 0;
  let applying: object | undefined;
  let message = "";
  let lastError = "";
  let checkedModelsFor: string | undefined;
  const lookups = new Map<string, string>();
  const positions = new Map<SetupMode, { focus?: string; scroll: number }>();
  const scrollParent = () => root.closest<HTMLElement>(".panel-content");
  root.addEventListener("focusin", (event) => {
    const key = (event.target as HTMLElement).dataset.genieFocus;
    if (key)
      positions.set(mode, {
        focus: key,
        scroll: scrollParent()?.scrollTop ?? window.scrollY,
      });
  });
  const connected = (accounts: SetupReview["ai_accounts"], id?: string) =>
    !!id && accounts[id]?.connected === true;
  const scopeFor = (id: string) =>
    state?.scopes.find((scope) => scope.repository_id === id);
  function progress() {
    if (!state) return [false, false, false, false];
    const { settings, readiness } = state.resources;
    const agentReady = (id: string) => {
      const agent = settings.agents?.find((a) => a.id === id);
      return (
        !!agent?.model.trim() &&
        connected(state!.ai_accounts, agent.ai_account?.account_id)
      );
    };
    const enabled = settings.repositories?.filter((r) => r.enabled) ?? [];
    return [
      Object.values(state.ai_accounts).some((a) => a.connected),
      Object.values(state.repository_accounts).some((a) => a.connected),
      settings.agents?.some((a) => agentReady(a.id)) ?? false,
      enabled.length > 0 &&
        readiness.configuration_ready &&
        enabled.every(
          (r) =>
            connected(state!.repository_accounts, r.provider_account_id) &&
            r.assignments?.length &&
            r.assignments.every((a) => agentReady(a.agent_id)) &&
            scopeFor(r.id)?.active,
        ),
    ];
  }
  function edit(target: SetupTarget, opener: HTMLElement) {
    positions.set(mode, {
      focus: opener.dataset.genieFocus,
      scroll: scrollParent()?.scrollTop ?? window.scrollY,
    });
    options.edit(target, opener, {
      back: () => {
        void open(mode);
      },
    });
  }
  function render() {
    if (!active) return;
    const focused = root.contains(document.activeElement)
      ? (document.activeElement as HTMLElement).dataset.genieFocus
      : undefined;
    const scrollParent = root.closest<HTMLElement>(".panel-content");
    const scroll = scrollParent?.scrollTop;
    const steps = progress();
    const count = steps.filter(Boolean).length;
    const next = steps.findIndex((ready) => !ready);
    options.title(
      mode === "welcome"
        ? "Welcome"
        : mode === "review"
          ? "Review setup"
          : "Genie",
    );
    const { settings } = state?.resources ?? {};
    const notice = message;
    root.innerHTML = `<p class="genie-error" role="alert" ${notice ? "" : "hidden"}>${escape(notice)}</p>
      ${
        mode === "review"
          ? reviewMarkup()
          : `
      <section class="genie-card"><div class="genie-card-heading"><span class="genie-symbol">${spark}</span><span>${mode === "welcome" ? "A fresh start" : "Onboarding Genie"}<small>${mode === "welcome" ? "" : count === 4 ? "Ready for your final check" : `Step ${next + 1} of 4`}</small></span></div>
      <h2>${mode === "welcome" ? "A second set of eyes.<br />Let's set yours up." : count === 4 ? "Your setup is ready." : titles[next]}</h2>
      <p>${mode === "welcome" ? "Connect your accounts, create a reviewer, and choose what to watch." : count === 4 ? "Review your saved identities, permissions, filters and global schedule. Repository Save already authorized monitoring." : guidance[next]}</p>
      <button type="button" class="genie-action" data-genie-next data-genie-focus="next" ${!state ? "disabled" : ""}>${mode === "welcome" ? (count ? "Continue with Genie" : "Set up with Genie") : count === 4 ? "Review setup" : titles[next]}</button>
      ${mode === "welcome" ? '<button type="button" class="genie-text" data-genie-manual data-genie-focus="manual">I’ll set it up myself</button>' : ""}
      </section>
      <div class="genie-progress"><span>Your setup</span><strong>${state ? `${count} of 4 ready` : "Reading saved setup..."}</strong><div role="progressbar" aria-label="Setup essentials ready" aria-valuemin="0" aria-valuemax="4" aria-valuenow="${count}">${steps.map((ready) => `<i class="${ready ? "complete" : ""}"></i>`).join("")}</div></div>
      <ol class="genie-checklist">${titles
        .map(
          (title, i) =>
            `<li><button type="button" data-genie-edit="${targets[i]}" data-genie-focus="${targets[i]}" ${!state ? "disabled" : ""}><span class="genie-step ${steps[i] ? "complete" : ""}">${steps[i] ? "✓" : i + 1}</span><span><strong>${title}</strong><small>${
              [
                steps[0]
                  ? "Verified AI identity; not a subscription or inference test"
                  : "Give your reviewers an AI account",
                steps[1]
                  ? "Repository identity connected independently"
                  : "Choose who accesses your repositories",
                steps[2]
                  ? "Saved account and model; catalog checked at final review"
                  : "Explicit account, model and shared principles",
                steps[3]
                  ? "Repository configuration saved and authorized"
                  : "Assign an Agent and save repository configuration",
              ][i]
            }</small></span><span aria-hidden="true">›</span></button></li>`,
        )
        .join("")}</ol>
      <p class="genie-note">Each repository save authorizes its configuration immediately. Closing keeps saved resources and mounted drafts. Back and Cancel explain unsaved fields in each editor. ${settings?.doctrines?.length ?? 0} shared doctrines available.</p>
      <button type="button" class="genie-text" data-genie-edit="doctrines" data-genie-focus="doctrines">Manage shared doctrines</button>
      <button type="button" class="genie-text" data-genie-edit="preferences" data-genie-focus="preferences">Edit global schedule and capacity</button>`
      }
      <button type="button" class="genie-text" data-genie-refresh data-genie-focus="refresh" ${applying ? "disabled" : ""}>Refresh saved setup</button>`;
    root
      .querySelector<HTMLButtonElement>("[data-genie-next]")
      ?.addEventListener("click", (event) => {
        if (mode === "welcome") void open("genie");
        else if (count === 4) void open("review");
        else edit(targets[next], event.currentTarget as HTMLElement);
      });
    root
      .querySelector<HTMLButtonElement>("[data-genie-manual]")
      ?.addEventListener("click", (event) =>
        options.manual(event.currentTarget as HTMLElement),
      );
    root.querySelector<HTMLButtonElement>("[data-genie-refresh]")!.onclick =
      () => void refresh(mode === "review");
    for (const button of root.querySelectorAll<HTMLButtonElement>(
      "[data-genie-edit]",
    ))
      button.onclick = () =>
        edit(button.dataset.genieEdit as SetupTarget, button);
    const activate = root.querySelector<HTMLButtonElement>(
      "[data-genie-activate]",
    );
    if (activate) {
      activate.onclick = () => void apply();
    }
    if (scrollParent && scroll !== undefined) scrollParent.scrollTop = scroll;
    if (focused)
      root
        .querySelector<HTMLElement>(`[data-genie-focus="${focused}"]`)
        ?.focus({ preventScroll: true });
  }
  function reviewMarkup() {
    if (!state)
      return '<section class="genie-card"><h2>Reading saved setup...</h2></section>';
    const { settings, readiness } = state.resources;
    const schedule = settings.defaults.schedule;
    const allReady =
      progress().every(Boolean) && checkedModelsFor === state.confirmation;
    return `<section class="genie-card"><span class="genie-review-label">${spark} Genie’s final check</span><h2>Your app. Your call.</h2><p>Monitoring does not grant extra review or publication permissions. Saved changes already apply to authorized work.</p></section>
      ${!allReady ? '<p class="genie-note" role="status">Complete all four essentials and check the current model catalogs before finishing setup.</p>' : ""}
      ${(settings.repositories ?? [])
        .map((repository) => {
          const scope = scopeFor(repository.id);
          const policy = effectivePolicy(
            settings.defaults,
            repository.overrides ?? {},
          );
          const watched = state!.watched_authors[repository.id];
          const authority = readiness.repositories.find(
            (r) => r.repository_id === repository.id,
          );
          const account =
            state!.repository_accounts[repository.provider_account_id ?? ""];
          return `<section class="genie-card genie-repository"><h3>${escape(repository.name)}</h3><dl>
          <dt>GitHub identity</dt><dd>${escape(account?.login ?? "Not connected")} (${escape(repository.provider_account_id ?? "unbound")})${account?.connected ? "" : " / reconnect required"}</dd>
          <dt>Repository ID</dt><dd>${escape(repository.provider_repository_id ?? "Not bound")}</dd>
          <dt>Monitoring</dt><dd>${!repository.enabled ? "Disabled; unchanged" : scope?.active ? "Authorized by saved configuration" : "Save repository configuration to authorize"}</dd>
          <dt>Pull requests</dt><dd>${scope?.mode === "all_open_and_future" ? "All currently open and future matching PRs" : scope?.active ? "Legacy saved admission retained; Save repository to include all currently open and future matching PRs" : "Not configured"}</dd>
          <dt>Authors</dt><dd>${watched.length ? escape(watched.map((a) => `${a.login} (${a.id})`).join(", ")) : "All authors"}</dd>
          <dt>Review requests</dt><dd>${policy.reviewer_assignment ? "Acting-account requests can admit older or unwatched PRs" : "Off"}</dd>
          <dt>Review execution</dt><dd>Automatic when eligible; pause, disablement, account access and capacity still apply</dd>
          <dt>Comment gate</dt><dd>${policy.automatic_comment_publication ? "Automatic only with assignment permission" : "Local-only until separately authorized"}</dd></dl>
          ${(repository.assignments ?? [])
            .map((assignment) => {
              const agent = settings.agents?.find(
                (a) => a.id === assignment.agent_id,
              );
              const ai =
                state!.ai_accounts[agent?.ai_account?.account_id ?? ""];
              const rights = authority?.assignments.find(
                ([id]) => id === assignment.id,
              )?.[1];
              return `<div class="genie-agent-summary"><strong>${escape(agent?.name ?? "Missing Agent")}${rights?.primary ? ` / Primary${repository.assignments?.length === 1 ? " (sole Agent)" : ""}` : ""}</strong><p>${escape(agent?.model ?? "No model")} / Copilot: ${escape(ai?.login ?? "Not connected")} (${escape(agent?.ai_account?.account_id ?? "unselected")})${ai?.connected ? "" : " / reconnect required"}</p><p>Comment: ${rights?.comment ? "allowed" : "off"} / Approve: ${rights?.approve ? "allowed" : "off"} / Merge: ${rights?.merge ? "allowed" : "off"}</p><p>Doctrines: ${escape(agent ? doctrineTitles(agent).join(", ") || "None" : "Unavailable")}</p></div>`;
            })
            .join("")}
          ${!authority?.primary_assignment_id ? '<p class="genie-note">No primary: automatic approval and merge are unavailable.</p>' : ""}
          <button class="genie-text" type="button" data-genie-edit="repositories" data-genie-focus="repository-${escape(repository.id)}">Edit repositories</button></section>`;
        })
        .join("")}
      <section class="genie-card"><h3>One global schedule</h3><p>${schedule.kind === "cron" ? `<code>${escape(schedule.expression)}</code>` : `Legacy interval: ${schedule.minutes} minutes; choose a global cron`} / ${escape(schedule.timezone)}</p>
      <h3>Room to work</h3><p>At most <strong>${settings.capacity} AI tasks</strong> on this computer. Full passes, primary final reviews and targeted replies share capacity.</p>
      <p>Global automation: <strong>${state.paused ? "Paused; finishing setup will not resume it" : "Running when saved configuration and execution gates allow"}</strong>.</p>
      <button type="button" class="genie-text" data-genie-edit="preferences" data-genie-focus="preferences">Edit schedule and capacity</button></section>
      <section class="genie-card"><p class="genie-note">Catalog availability is not a subscription, seat or inference test. No review runs in this check. Repository Save already authorized monitoring; there is no further repository confirmation.</p>
      <button type="button" class="genie-action" data-genie-activate data-genie-focus="activate" ${!allReady || applying ? "disabled" : ""}>${applying ? "Checking current setup..." : "Finish setup"}</button></section>`;
  }
  async function read() {
    // Restore native account metadata through the existing widgets' API. Neither
    // role is selected or connected by this read.
    await Promise.all([
      invoke("github_auth_state"),
      invoke("copilot_auth_state"),
    ]);
    return invoke<SetupReview>("monitoring_setup_review");
  }
  async function checkModels(review: SetupReview, current: number) {
    const settings = review.resources.settings;
    const assigned = new Set(
      settings.repositories
        ?.filter((r) => r.enabled)
        .flatMap((r) => r.assignments?.map((a) => a.agent_id) ?? []),
    );
    const agents = settings.agents?.filter((a) => assigned.has(a.id)) ?? [];
    const accounts = new Set(
      agents
        .map((a) => a.ai_account?.account_id)
        .filter((id): id is string => !!id),
    );
    for (const accountId of accounts) {
      if (!connected(review.ai_accounts, accountId))
        throw "Reconnect the assigned AI account before the final check.";
      const requestId = crypto.randomUUID();
      lookups.set(requestId, accountId);
      let models: CopilotModel[];
      try {
        models = await invoke<CopilotModel[]>("list_copilot_models", {
          accountId,
          requestId,
        });
      } finally {
        lookups.delete(requestId);
      }
      if (!active || current !== request) return false;
      if (
        agents.some(
          (a) =>
            a.ai_account?.account_id === accountId &&
            !models.some((m) => m.id === a.model && modelSelectable(m)),
        )
      )
        throw "An assigned model is unavailable in its account's current catalog. Edit the Agent or retry; no replacement was selected.";
      for (const agent of agents.filter(
        (a) => a.ai_account?.account_id === accountId,
      )) {
        const invalid = intelligenceError(
          models.find((model) => model.id === agent.model),
          agent.intelligence,
        );
        if (invalid)
          throw `${agent.name}: ${invalid} Edit the Agent; no replacement was selected.`;
      }
    }
    return true;
  }
  async function refresh(models = false) {
    if (!active || applying) return;
    const current = ++request;
    if (models) checkedModelsFor = undefined;
    try {
      const next = await read();
      if (!active || current !== request) return;
      const changed = next.confirmation !== state?.confirmation;
      state = next;
      options.state?.(next);
      if (changed) checkedModelsFor = undefined;
      message = "";
      if (models && progress().every(Boolean)) {
        render();
        if (!(await checkModels(next, current))) return;
        const latest = await read();
        if (!active || current !== request) return;
        if (latest.confirmation !== next.confirmation)
          throw "Setup changed during the model check. Refresh and review it again.";
        checkedModelsFor = next.confirmation;
      }
      if (changed || models || lastError || !root.children.length) render();
      lastError = "";
    } catch (cause) {
      if (active && current === request) {
        checkedModelsFor = undefined;
        message = failure(cause);
        lastError = message;
        render();
      }
    }
  }
  async function apply() {
    if (!state || applying) return;
    const expected = state;
    const current = ++request;
    const operation = {};
    applying = operation;
    message = "";
    render();
    try {
      if (!(await checkModels(expected, current))) return;
      const latest = await read();
      if (!active || current !== request) return;
      if (latest.confirmation !== expected.confirmation)
        throw "Setup changed. Refresh and review the current configuration before finishing setup.";
      if (active && current === request) options.complete(expected.paused);
    } catch (cause) {
      if (active && current === request) {
        message = failure(cause);
        checkedModelsFor = undefined;
      }
    } finally {
      if (applying === operation) {
        applying = undefined;
        if (active && current === request) render();
        else if (active) void refresh();
      }
    }
  }
  async function open(next: SetupMode = "genie") {
    invalidateWork();
    mode = next;
    active = true;
    render();
    const position = positions.get(mode);
    if (position?.focus)
      root
        .querySelector<HTMLElement>(
          `[data-genie-focus="${CSS.escape(position.focus)}"]`,
        )
        ?.focus({ preventScroll: true });
    const parent = scrollParent();
    if (parent) parent.scrollTop = position?.scroll ?? 0;
    else window.scrollTo(0, position?.scroll ?? 0);
    await refresh(mode === "review");
  }
  function leave() {
    positions.set(mode, {
      focus: positions.get(mode)?.focus,
      scroll: scrollParent()?.scrollTop ?? window.scrollY,
    });
    active = false;
    invalidateWork();
  }
  function invalidateWork() {
    request++;
    checkedModelsFor = undefined;
    applying = undefined;
    for (const [requestId, accountId] of lookups)
      void invoke("cancel_copilot_models", { accountId, requestId }).catch(
        (cause) => {
          message = `Model lookup cancellation could not be confirmed. ${failure(cause)}`;
          console.error(message);
          if (active) render();
        },
      );
  }
  window.addEventListener("focus", () => {
    if (active) void refresh();
  });
  return {
    open,
    leave,
    refresh,
    back: () => open(mode === "review" ? "genie" : "welcome"),
  };
}

/** The legacy Settings URL shares the same mounted editors, too. */
export async function mountSettingsWithGenie(app: HTMLElement) {
  app.className = "legacy-settings-host";
  const settings = document.createElement("div");
  const setup = document.createElement("section");
  setup.className = "legacy-genie";
  setup.hidden = true;
  setup.innerHTML =
    '<button type="button" data-legacy-back>Back to Settings</button><h1 tabindex="-1">Genie</h1><div data-legacy-genie></div>';
  app.replaceChildren(settings, setup);
  let opener: HTMLElement | undefined;
  let scroll = 0;
  const showSettings = () => {
    genie.leave();
    setup.hidden = true;
    settings.hidden = false;
    window.scrollTo(0, scroll);
    if (opener?.isConnected) opener.focus({ preventScroll: true });
  };
  const showGenie = () => {
    settings.hidden = true;
    setup.hidden = false;
    setup.querySelector("h1")!.focus();
  };
  const genie = mountGenie(setup.querySelector("[data-legacy-genie]")!, {
    title: (value) => {
      setup.querySelector("h1")!.textContent = value;
    },
    manual: showSettings,
    complete: showSettings,
    edit: (target, _button, origin) => {
      showSettings();
      controller.open(target, {
        back: () => {
          showGenie();
          origin.back();
        },
      });
    },
  });
  const controller = await mountSettings(settings, {
    openGenie: (button) => {
      opener = button;
      scroll = window.scrollY;
      showGenie();
      void genie.open();
    },
  });
  setup.querySelector<HTMLButtonElement>("[data-legacy-back]")!.onclick =
    () => {
      controller.leaveGuidance();
      showSettings();
    };
}
