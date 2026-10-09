import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mountSettings } from "./settings";
import { mountGenie } from "./genie";
import {
  type Diagnostic,
  diagnosticLog,
  diagnosticNotice,
} from "./diagnostics";
import {
  type SetupReview,
  type DoctrineCatalog,
  doctrineCatalogLabel,
} from "./resources";
import { renderMonitoring, type MonitoringSnapshot } from "./monitoring";
import type { AutomationSnapshot } from "./automation";
import crosshair from "./crosshair.svg";
import sniperArt from "./sniper-mark.png";
import { humanQueue } from "./queue";
import {
  workPresentation,
  purposes,
  type WorkPresentation,
  renderIntelligenceDiagnostics,
} from "./work-presentation";
import "./panel.css";

export type PanelTab = "queue" | "running" | "reviewed" | "settings";
export type WorkKind = "normal" | "reply" | "mention" | "primary_final";
export type PanelDetail =
  | { type: "item"; item_id: string }
  | { type: "job"; kind: WorkKind; id: string }
  | { type: "status" | "diagnostics" };
export interface PanelRoute {
  tab: PanelTab;
  detail?: PanelDetail;
}
interface PanelSnapshot {
  route: PanelRoute;
  revision: number;
  visible: boolean;
  missing: string | null;
  placement_warning: string | null;
}
interface Position {
  scroll: number;
  focus: HTMLElement | null;
  nested: [HTMLElement, number][];
  row?: { type: "item" | "job"; id: string };
}
interface Row {
  id: string;
  kind: WorkKind | "item";
  title: string;
  state: string;
  reason?: string | null;
  work?: WorkPresentation;
}

const destinations = {
  queue: {
    title: "Your queue",
    icon: "M4 4h16l2 14H2L4 4Zm-1 9h5l2 3h4l2-3h5M7 8h10",
  },
  running: { title: "Work queue", icon: "M2 12h4l3-9 5 18 3-9h5" },
  reviewed: {
    title: "Reviewed",
    icon: "M7 3h10a2 2 0 0 1 2 2v15H5V5a2 2 0 0 1 2-2Zm1 9 3 3 5-6",
  },
  settings: {
    title: "Settings",
    icon: "M3 6h18M3 12h18M3 18h18M8 3v6M16 9v6M9 15v6",
  },
};
const icon = (path: string) =>
  `<svg class="panel-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="${path}"/></svg>`;

export async function mountPanel(app: HTMLElement) {
  app.className = "panel-shell";
  app.innerHTML = `<header class="panel-header"><img src="${crosshair}" alt="" /><strong>PR Sniper</strong><span data-setup-needed hidden>Setup needed</span><div data-header-automation></div><button type="button" data-panel-hide aria-label="Hide PR Sniper panel" title="Close hides only; background work continues">${icon("m7 7 10 10M7 17 17 7")}</button></header>
    <div class="panel-context"><button type="button" data-panel-back aria-label="Back" hidden>${icon("m14 6-6 6 6 6M8 12h12")}</button><h1 tabindex="-1" data-panel-heading>Your queue</h1><img class="panel-art" src="${sniperArt}" alt="" /></div>
    <div class="panel-summary" data-panel-summary>
      <div data-work-summary><strong data-summary-main>Reading queue...</strong></div>
      <dl class="queue-metrics" aria-label="Human handoffs" data-queue-metrics hidden>
        <div class="queue-metric" data-queue-ready-card><dt>Ready for you${icon(destinations.queue.icon)}</dt><dd><strong data-queue-ready>?</strong><span>need your eyes</span></dd></div>
        <div class="queue-metric" data-queue-attention-card><dt>Needs attention${icon("M12 8v5m0 3h.01M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18")}</dt><dd><strong data-queue-attention>?</strong><span>need your input</span></dd></div>
      </dl>
      <button type="button" class="queue-work-link" data-queue-work hidden aria-label="View Agent work in Running"><span class="queue-capacity" data-queue-capacity aria-hidden="true">?</span><span><strong>Agent work in Running</strong><span data-queue-work-status>Work state unavailable</span></span>${icon("m9 5 7 7-7 7")}</button>
      <div class="queue-list-heading"><h2 data-queue-heading hidden>Over to you</h2><span data-summary-detail></span></div>
    </div>
    <p class="panel-error" role="alert" data-panel-error hidden></p>
    <div class="panel-content">
      <div data-panel-view="monitor"></div>
      <section data-panel-view="running" hidden><div data-running-list></div></section>
      <section data-panel-view="reviewed" hidden></section>
      <div data-panel-view="settings" hidden></div>
      <div data-panel-view="genie" hidden></div>
      <section data-panel-view="utility" hidden></section>
    </div>
    <nav class="panel-tabs" data-panel-navigation aria-label="Application destinations">${(
      ["queue", "running", "reviewed", "settings"] as const
    )
      .map(
        (tab) =>
          `<button type="button" data-panel-tab="${tab}" aria-label="${tab[0].toUpperCase() + tab.slice(1)}"><span class="nav-icon">${icon(destinations[tab].icon)}${tab === "running" ? '<span data-running-count aria-hidden="true">?</span>' : ""}</span><span>${tab[0].toUpperCase() + tab.slice(1)}</span></button>`,
      )
      .join("")}</nav>
    <footer class="panel-footer"><button type="button" data-panel-status>Status</button><button type="button" data-panel-diagnostics>Diagnostics</button><span data-panel-version role="status" aria-label="Application version">Reading version...</span><span class="panel-hide-hint">Close hides only. Quit from the tray menu.</span></footer>`;
  const version = app.querySelector<HTMLElement>("[data-panel-version]")!;
  let versionKnown = false;
  const showVersion = (value: unknown, failure?: unknown) => {
    if (typeof value === "string" && value.trim()) {
      versionKnown = true;
      version.textContent = `v${value.trim()}`;
      version.title = `Running PR Sniper ${value.trim()}`;
    } else if (!versionKnown) {
      version.textContent = "Version unavailable";
      version.title = `${failure === undefined ? "Native application metadata did not include a version." : `Native metadata unavailable: ${String(failure)}`} Open Status to retry.`;
    }
  };
  void invoke<{ version: unknown }>("snapshot").then(
    (state) => showVersion(state.version),
    (cause) => showVersion(undefined, cause),
  );
  const error = app.querySelector<HTMLElement>("[data-panel-error]")!;
  const heading = app.querySelector<HTMLElement>("[data-panel-heading]")!;
  const content = app.querySelector<HTMLElement>(".panel-content")!;
  const context = app.querySelector<HTMLElement>(".panel-context")!;
  const summary = app.querySelector<HTMLElement>("[data-panel-summary]")!;
  const back = app.querySelector<HTMLButtonElement>("[data-panel-back]")!;
  const views = {
    monitor: app.querySelector<HTMLElement>('[data-panel-view="monitor"]')!,
    running: app.querySelector<HTMLElement>('[data-panel-view="running"]')!,
    reviewed: app.querySelector<HTMLElement>('[data-panel-view="reviewed"]')!,
    settings: app.querySelector<HTMLElement>('[data-panel-view="settings"]')!,
    genie: app.querySelector<HTMLElement>('[data-panel-view="genie"]')!,
    utility: app.querySelector<HTMLElement>('[data-panel-view="utility"]')!,
  };
  let route: PanelRoute = { tab: "queue" };
  let revision = -1;
  let returnTo: PanelRoute = { tab: "queue" };
  let settingsController: ReturnType<typeof mountSettings> | undefined;
  let settingsNavigation: Awaited<ReturnType<typeof mountSettings>> | undefined;
  let setupOwner: PanelTab | undefined;
  let setupTitle = "Welcome";
  let intent = 0;
  let snapshot: MonitoringSnapshot | undefined;
  let automation: AutomationSnapshot | undefined;
  let navigating = Promise.resolve();
  let navigationOrigin: string | undefined;
  let utilityRevision = 0;
  let lastVisible = false;
  let focusRevision = 0;
  let setupRead = 0;
  const positions = new Map<string, Position>();
  const listSignatures = new Map<PanelTab, string>();
  const key = (value: PanelRoute) => JSON.stringify(value);
  const positionKey = () =>
    setupVisible() ? `${key(route)}:genie` : key(route);
  const setupVisible = () => setupOwner === route.tab && !route.detail;
  const showError = (message: string) => {
    error.textContent = message;
    error.hidden = false;
  };
  const remember = (opener?: HTMLElement) => {
    if (!opener && navigationOrigin === key(route)) return;
    const previous = positions.get(positionKey());
    const focus =
      opener ??
      (document.activeElement instanceof HTMLElement &&
      content.contains(document.activeElement)
        ? document.activeElement
        : (previous?.focus ?? null));
    const row = focus?.closest<HTMLElement>("[data-item-id],[data-job-id]");
    const nested = [...content.querySelectorAll<HTMLElement>("*")]
      .filter(
        (element) =>
          element.clientHeight > 0 &&
          element.scrollHeight > element.clientHeight &&
          ["auto", "scroll"].includes(getComputedStyle(element).overflowY),
      )
      .map((element): [HTMLElement, number] => [element, element.scrollTop]);
    positions.set(positionKey(), {
      scroll: content.scrollTop,
      focus,
      nested,
      row: row?.dataset.itemId
        ? { type: "item", id: row.dataset.itemId }
        : row?.dataset.jobId
          ? { type: "job", id: row.dataset.jobId }
          : previous?.row,
    });
  };
  content.addEventListener("focusin", () => remember());
  app.addEventListener("focusin", () => focusRevision++);
  function restorePosition(focus: boolean) {
    const saved = positions.get(positionKey());
    content.scrollTop = saved?.scroll ?? 0;
    for (const [element, scroll] of saved?.nested ?? [])
      if (element.isConnected && !element.closest("[hidden]"))
        element.scrollTop = scroll;
    if (!focus) return;
    const oldTarget = saved?.focus;
    const target =
      setupVisible() && oldTarget?.dataset.genieFocus
        ? views.genie.querySelector<HTMLElement>(
            `[data-genie-focus="${CSS.escape(oldTarget.dataset.genieFocus)}"]`,
          )
        : oldTarget;
    if (target?.isConnected && !target.closest("[hidden]"))
      target.focus({ preventScroll: true });
    else if (saved?.row) {
      const row = app.querySelector<HTMLElement>(
        `[data-${saved.row.type}-id="${CSS.escape(saved.row.id)}"]`,
      );
      (row?.querySelector<HTMLElement>("button") ?? heading).focus({
        preventScroll: true,
      });
      if (row) {
        const bounds = row.getBoundingClientRect();
        const viewport = content.getBoundingClientRect();
        if (bounds.bottom <= viewport.top || bounds.top >= viewport.bottom)
          row.scrollIntoView({ block: "nearest" });
      }
    } else heading.focus({ preventScroll: true });
  }
  const monitor = renderMonitoring(views.monitor, showError, {
    panel: true,
    navigate: (detail, opener) =>
      void navigate(
        {
          tab: route.tab === "settings" ? "queue" : route.tab,
          detail,
        },
        opener,
      ),
    onSnapshot: (value) => {
      snapshot = value;
      drawSummary();
      drawLists();
    },
    onAutomation: (value) => {
      automation = value;
      app.dataset.automationUnavailable = String(!value);
      app.querySelector<HTMLElement>("[data-running-count]")!.textContent =
        value ? String(value.active) : "?";
      app.querySelector<HTMLButtonElement>(
        '[data-panel-tab="running"]',
      )!.title = value
        ? `${value.active} occupied AI slots, including ${value.stopping} stopping`
        : "Active work unavailable";
      drawSummary();
      drawLists();
      const read = ++setupRead;
      const owner = intent;
      const destination = key(route);
      const current = () =>
        read === setupRead && owner === intent && destination === key(route);
      void invoke<SetupReview>("monitoring_setup_review").then(
        (value) => {
          if (current()) setupState(value);
        },
        (cause) => {
          if (!current()) return;
          showError(
            typeof cause === "string"
              ? cause
              : "Monitoring setup status is unavailable.",
          );
        },
      );
    },
  });
  monitor.automation(
    app.querySelector<HTMLElement>("[data-header-automation]")!,
  );
  const needsSetup = (value: SetupReview, work: MonitoringSnapshot) =>
    !value.configured_inactive &&
    !value.scopes.some((scope) => scope.active) &&
    Array.isArray(work.items) &&
    !work.items.length;
  const setupState = (value: SetupReview) => {
    setupRead++;
    if (!snapshot?.items) return;
    const empty = needsSetup(value, snapshot);
    app.dataset.setupEmpty = String(empty);
    app.querySelector<HTMLElement>("[data-setup-needed]")!.hidden = !empty;
  };
  const ensureSettings = () =>
    (settingsController ??= mountSettings(views.settings, {
      embedded: true,
      openGenie: (opener) => void showSetup("settings", opener),
    }).then((controller) => (settingsNavigation = controller)));
  const genie = mountGenie(views.genie, {
    title: (value) => {
      setupTitle = value;
      if (setupVisible()) {
        heading.textContent = value;
        back.hidden = value === "Welcome";
      }
    },
    state: setupState,
    manual: (opener) => {
      remember(opener);
      setupOwner = undefined;
      genie.leave();
      void navigate({ tab: "settings" });
    },
    complete: (paused) => {
      setupOwner = undefined;
      genie.leave();
      app.dataset.setupEmpty = "false";
      app.querySelector<HTMLElement>("[data-setup-needed]")!.hidden = true;
      const navigation = navigate({ tab: "queue" });
      const expected = intent;
      void navigation.then(() => {
        if (paused && intent === expected)
          showError(
            "Monitoring scope confirmed. Global automation remains paused; resume it explicitly in Settings.",
          );
        void monitor.refresh();
      });
    },
    edit: (target, opener, origin) => {
      const owner = setupOwner ?? "settings";
      remember(opener);
      setupOwner = undefined;
      genie.leave();
      const task = navigate({ tab: "settings" });
      const expected = intent;
      void task.then(async () => {
        const controller = await ensureSettings();
        if (intent !== expected || route.tab !== "settings" || route.detail)
          return;
        controller.open(target, {
          back: () => void showSetup(owner, undefined, origin.back),
        });
      });
    },
  });
  async function showSetup(
    owner: PanelTab,
    opener?: HTMLElement,
    reopen?: () => void,
  ) {
    remember(opener);
    setupOwner = owner;
    const navigation = navigate({ tab: owner });
    const expected = intent;
    await navigation;
    if (intent !== expected || !setupVisible()) return;
    if (reopen) reopen();
    else await genie.open("genie");
  }

  function drawSummary() {
    const summary = app.querySelector<HTMLElement>("[data-panel-summary]")!;
    const queue = route.tab === "queue";
    summary.dataset.summaryKind = queue ? "queue" : "work";
    summary.querySelector<HTMLElement>("[data-queue-metrics]")!.hidden = !queue;
    summary.querySelector<HTMLElement>("[data-queue-work]")!.hidden = !queue;
    summary.querySelector<HTMLElement>("[data-queue-heading]")!.hidden = !queue;
    summary.querySelector<HTMLElement>("[data-work-summary]")!.className = queue
      ? "sr-only"
      : "";
    summary.hidden =
      setupVisible() ||
      !!route.detail ||
      ["settings", "reviewed"].includes(route.tab);
    const main = summary.querySelector<HTMLElement>("[data-summary-main]")!;
    const detail = summary.querySelector<HTMLElement>("[data-summary-detail]")!;
    if (route.tab === "running") {
      main.textContent = automation
        ? `${automation.active - automation.stopping} / ${automation.capacity} running`
        : "Work state unavailable";
      detail.textContent = automation
        ? `${automation.waiting} waiting${automation.blocked ? ` / ${automation.blocked} blocked` : ""}${automation.stopping ? ` / ${automation.stopping} stopping` : ""}`
        : "Occupancy unknown";
    } else {
      const items = humanQueue(snapshot?.items ?? []);
      const known = Array.isArray(snapshot?.items);
      const ready = items.filter(
        (item) => item.state === "machine_signed_off",
      ).length;
      main.textContent = known
        ? `${items.length} for you`
        : "Queue unavailable";
      detail.textContent = known
        ? `${ready} ready / ${items.length - ready} need attention`
        : "Handoffs unavailable";
      summary.querySelector<HTMLElement>("[data-queue-ready]")!.textContent =
        known ? String(ready) : "?";
      summary.querySelector<HTMLElement>(
        "[data-queue-attention]",
      )!.textContent = known ? String(items.length - ready) : "?";
      summary.querySelector<HTMLElement>("[data-queue-capacity]")!.textContent =
        automation ? `${automation.active}/${automation.capacity}` : "?";
      summary.querySelector<HTMLElement>(
        "[data-queue-work-status]",
      )!.textContent = automation
        ? `${automation.active - automation.stopping} running${automation.stopping ? ` / ${automation.stopping} stopping` : ""} / ${automation.waiting} waiting${automation.blocked ? ` / ${automation.blocked} blocked` : ""}${automation.paused ? " / paused" : ""}`
        : "Work state unavailable";
    }
  }
  function metadata(
    kind: WorkKind,
    id: string,
  ): { title: string; state: string } | undefined {
    const work = snapshot && workPresentation(snapshot, kind, id);
    return work
      ? { title: `${work.reference} / ${work.agent}`, state: work.state }
      : undefined;
  }
  function draw(tab: "running" | "reviewed", rows: Row[]) {
    const signature = JSON.stringify(rows);
    if (listSignatures.get(tab) === signature) return;
    listSignatures.set(tab, signature);
    const root =
      tab === "running"
        ? views.running.querySelector<HTMLElement>("[data-running-list]")!
        : views.reviewed;
    const activeId =
      document.activeElement instanceof HTMLElement
        ? document.activeElement.closest<HTMLElement>("[data-job-id]")?.dataset
            .jobId
        : undefined;
    root.replaceChildren();
    const hint = document.createElement("p");
    hint.textContent =
      tab === "running"
        ? "Running at the top. New work joins the bottom."
        : "Completed evidence available in the current native projection. This foundation does not add paged history or storage purge.";
    root.append(hint);
    hint.className = "work-caption";
    const list = document.createElement("ol");
    if (tab === "running") {
      list.className = "work-list";
      list.setAttribute("aria-label", "Agent work queue");
      root.append(list);
    }
    if (!rows.length) {
      const empty = document.createElement("p");
      empty.textContent =
        tab === "running"
          ? "No active, waiting or blocked AI jobs."
          : "No completed evidence is available yet.";
      root.append(empty);
    }
    for (const row of rows) {
      const article = document.createElement("article");
      article.dataset.jobId = `${row.kind}:${row.id}`;
      article.dataset.workState = row.state;
      const title = document.createElement("h2");
      title.textContent = row.title;
      const status = document.createElement("p");
      status.textContent = `${row.kind.replaceAll("_", " ")}: ${row.state}. ${row.reason ?? ""}`;
      const button = document.createElement("button");
      button.type = "button";
      button.textContent =
        row.kind === "item" ? "Open PR evidence" : "Open job";
      button.onclick = () =>
        void navigate(
          {
            tab,
            detail:
              row.kind === "item"
                ? { type: "item", item_id: row.id }
                : { type: "job", kind: row.kind, id: row.id },
          },
          button,
        );
      if (tab === "running") {
        const displayState =
          row.state === "active"
            ? "Running"
            : row.state === "stopping"
              ? "Stopping"
              : row.work?.state === "superseded"
                ? "Superseded"
                : ["failed", "manual_retry"].includes(row.work?.state ?? "")
                  ? "Failed"
                  : row.state === "blocked"
                    ? "Blocked"
                    : "Queued";
        button.className = "work-row";
        button.setAttribute("aria-label", "Open job");
        button.title = `${row.title}; ${displayState}`;
        button.replaceChildren();
        const marker = document.createElement("span");
        marker.className = row.state === "active" ? "work-spin" : "work-marker";
        marker.setAttribute("aria-hidden", "true");
        if (row.state !== "active")
          marker.innerHTML = icon(destinations.queue.icon);
        const copy = document.createElement("span");
        copy.className = "work-copy";
        const top = document.createElement("span");
        top.className = "work-heading";
        const agent = document.createElement("strong");
        agent.textContent = row.work?.agent ?? "Job evidence unavailable";
        const state = document.createElement("span");
        state.className = "work-state";
        state.textContent = displayState;
        top.append(agent, state);
        const subject = document.createElement("span");
        subject.className = "work-subject";
        subject.textContent = row.work?.subject ?? row.title;
        const reference = document.createElement("span");
        reference.className = "work-reference";
        reference.textContent = row.work?.reference ?? "Repository unavailable";
        const purpose = document.createElement("span");
        purpose.textContent =
          row.work?.ordinal ??
          (row.kind === "item" ? "PR" : purposes[row.kind]);
        reference.append(purpose);
        copy.append(top, subject, reference);
        if (row.reason) {
          const reason = document.createElement("span");
          reason.className = "work-blocker";
          reason.textContent = row.reason;
          copy.append(reason);
        }
        const chevron = document.createElement("span");
        chevron.className = "work-chevron";
        chevron.setAttribute("aria-hidden", "true");
        button.append(marker, copy, chevron);
        article.append(button);
        const entry = document.createElement("li");
        entry.append(article);
        list.append(entry);
      } else {
        article.append(title, status, button);
        root.append(article);
      }
      if (
        activeId === article.dataset.jobId &&
        route.tab === tab &&
        !route.detail
      ) {
        button.focus({ preventScroll: true });
        const saved = positions.get(key(route));
        if (saved) saved.focus = button;
      }
    }
  }
  function drawLists() {
    if (!automation) {
      views.running.querySelector<HTMLElement>(
        "[data-running-list]",
      )!.textContent =
        "Work state unavailable. No active or waiting jobs can be confirmed. Retry from Status or inspect Diagnostics.";
      listSignatures.delete("running");
    } else
      draw(
        "running",
        [...automation.work]
          .sort((a, b) => {
            const rank = (state: string) =>
              state === "active" ? 0 : state === "stopping" ? 1 : 2;
            return (
              rank(a.state) - rank(b.state) || a.enqueue_order - b.enqueue_order
            );
          })
          .map((work) => ({
            id: work.key.id,
            kind: work.key.kind,
            title:
              metadata(work.key.kind, work.key.id)?.title ??
              `Saved ${work.key.kind} job ${work.key.id}`,
            state: work.state,
            reason:
              work.reason ??
              (automation?.paused && work.state === "waiting"
                ? "Monitoring paused."
                : null),
            work:
              snapshot &&
              workPresentation(snapshot, work.key.kind, work.key.id),
          })),
      );
    if (!snapshot) {
      views.reviewed.textContent =
        "Saved evidence unavailable; no completed state is inferred.";
      listSignatures.delete("reviewed");
      return;
    }
    const rows: Row[] = [];
    for (const review of snapshot.reviews ?? [])
      if (review.run?.operation.state === "completed") {
        rows.push({
          id: review.key,
          kind: "normal",
          title: metadata("normal", review.key)!.title,
          state: review.run.phase,
        });
      }
    for (const follow of snapshot.follow_ups ?? [])
      if (follow.run.result) {
        const kind =
          follow.run.target?.kind === "mention" ? "mention" : "reply";
        const meta = metadata(kind, follow.run.id);
        if (meta)
          rows.push({
            id: follow.run.id,
            kind,
            title: meta.title,
            state: follow.run.phase,
          });
      }
    for (const item of snapshot.items ?? []) {
      const final = item.action_status?.final_review;
      if (final?.execution.operation.state === "completed")
        rows.push({
          id: final.id,
          kind: "primary_final",
          title: `${item.job.repository_name} #${item.job.number} / Primary final review`,
          state: final.execution.phase,
        });
      if (["closed", "merged"].includes(item.state)) {
        rows.push({
          id: item.id,
          kind: "item",
          title: `${item.job.repository_name} #${item.job.number}: ${item.job.title}`,
          state: item.state,
        });
      }
    }
    draw("reviewed", rows);
  }
  async function utility(type: "status" | "diagnostics") {
    const request = ++utilityRevision;
    views.utility.replaceChildren();
    try {
      const state = await invoke<{
        version: string;
        isolated: boolean;
        error: string | null;
        doctrine_catalog?: DoctrineCatalog | null;
      }>("snapshot");
      if (request !== utilityRevision) return;
      showVersion(state.version);
      if (state.error) showError(state.error);
      const info = document.createElement("p");
      info.textContent = `PR Sniper ${state.version}. ${state.isolated ? "Isolated profile." : "Native host."} Hiding the panel does not stop background work.`;
      views.utility.append(info);
      const refresh = document.createElement("button");
      refresh.textContent = `Refresh ${type}`;
      refresh.onclick = () => void utility(type);
      views.utility.append(refresh);
      if (type === "diagnostics") {
        const [entries, jobs] = await Promise.all([
          invoke<Diagnostic[]>("diagnostics"),
          invoke<MonitoringSnapshot>("monitoring_snapshot"),
        ]);
        if (request !== utilityRevision) return;
        const pre = document.createElement("pre");
        const notice = document.createElement("p");
        notice.textContent = diagnosticNotice;
        views.utility.append(notice);
        pre.textContent =
          `${doctrineCatalogLabel(state.doctrine_catalog)}\n${JSON.stringify(state.doctrine_catalog ?? null, null, 2)}\n\n` +
          diagnosticLog(entries);
        views.utility.append(pre);
        renderIntelligenceDiagnostics(views.utility, jobs);
      } else {
        const tools = document.createElement("div");
        tools.dataset.panelRecovery = "true";
        views.utility.append(tools);
        monitor.tools(tools);
      }
    } catch (cause) {
      if (request === utilityRevision)
        showError(
          typeof cause === "string"
            ? cause
            : "Native status/diagnostics could not be read.",
        );
    }
  }
  async function apply(state: PanelSnapshot) {
    if (state.revision < revision) return;
    const changed = key(state.route) !== key(route);
    const restore = state.visible && (changed || !lastVisible);
    const focusAtStart = focusRevision;
    lastVisible = state.visible;
    if (changed || !state.visible) remember();
    if (changed && (state.route.tab !== setupOwner || state.route.detail)) {
      if (setupOwner !== undefined) genie.leave();
      setupOwner = undefined;
    }
    if (changed && (state.route.tab !== "settings" || state.route.detail))
      settingsNavigation?.leaveGuidance();
    if (changed && state.route.detail && !route.detail) returnTo = route;
    if (
      changed &&
      state.route.detail &&
      route.detail &&
      state.route.tab !== route.tab
    )
      returnTo = { tab: state.route.tab };
    route = state.route;
    revision = state.revision;
    app.dataset.nativeVisible = String(state.visible);
    if (state.placement_warning) showError(state.placement_warning);
    back.hidden =
      !route.detail && (!setupVisible() || setupTitle === "Welcome");
    heading.textContent = setupVisible()
      ? setupTitle
      : route.detail
        ? route.detail.type === "status"
          ? "Status"
          : route.detail.type === "diagnostics"
            ? "Diagnostics"
            : route.detail.type === "job"
              ? "Job details"
              : "Saved evidence"
        : destinations[route.tab].title;
    app.dataset.detail = String(!!route.detail);
    app.dataset.evidence = route.detail?.type ?? "";
    app.dataset.setup = String(setupVisible());
    const scrollQueue = route.tab === "queue" && !setupVisible();
    app.dataset.scrollQueue = String(scrollQueue);
    if (scrollQueue) {
      content.tabIndex = 0;
      content.setAttribute("role", "region");
      content.setAttribute("aria-label", "Queue content");
    } else {
      content.removeAttribute("tabindex");
      content.removeAttribute("role");
      content.removeAttribute("aria-label");
    }
    // Queue chrome shares the evidence scroll budget; navigation stays pinned.
    if (scrollQueue && context.parentElement !== content)
      content.prepend(context, summary, error);
    else if (!scrollQueue && context.parentElement === content)
      content.before(context, summary, error);
    drawSummary();
    for (const button of app.querySelectorAll<HTMLButtonElement>(
      "[data-panel-tab]",
    )) {
      if (button.dataset.panelTab === route.tab)
        button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    const utilityDetail =
      route.detail?.type === "status" || route.detail?.type === "diagnostics";
    const visible = setupVisible()
      ? "genie"
      : utilityDetail
        ? "utility"
        : route.detail || route.tab === "queue"
          ? "monitor"
          : route.tab;
    for (const [name, view] of Object.entries(views))
      view.hidden = name !== visible;
    views.settings.dispatchEvent(
      new CustomEvent("pr-sniper:section-active", {
        detail: visible === "settings",
      }),
    );
    if (visible === "settings" && !settingsController) {
      await ensureSettings();
      if (state.revision !== revision) return;
    }
    if (visible === "monitor") monitor.detail(route.detail, state.missing);
    if (utilityDetail && (changed || !views.utility.children.length))
      void utility(route.detail!.type as "status" | "diagnostics");
    if (restore && focusAtStart === focusRevision) {
      // Commit route focus now, before a subsequent explicit focus can win.
      // Only layout positioning waits for a frame; it cannot own newer input.
      restorePosition(true);
      const focused = focusRevision;
      const destination = key(route);
      requestAnimationFrame(() => {
        if (
          state.revision === revision &&
          destination === key(route) &&
          lastVisible &&
          focused === focusRevision
        )
          restorePosition(false);
      });
    }
  }
  function navigate(next: PanelRoute, opener?: HTMLElement) {
    intent++;
    // Capture activation before a queue redraw detaches the actual row button.
    remember(opener);
    navigating = navigating.then(async () => {
      navigationOrigin = key(route);
      error.hidden = true;
      try {
        await apply(
          await invoke<PanelSnapshot>("panel_navigate", { route: next }),
        );
      } catch (cause) {
        showError(
          typeof cause === "string"
            ? cause
            : "Panel navigation failed; the current view was retained.",
        );
      } finally {
        navigationOrigin = undefined;
      }
    });
    return navigating;
  }
  back.onclick = () => {
    if (setupVisible()) void genie.back();
    else void navigate(returnTo.detail ? { tab: route.tab } : returnTo);
  };
  for (const button of app.querySelectorAll<HTMLButtonElement>(
    "[data-panel-tab]",
  ))
    button.onclick = () => {
      if (setupVisible()) {
        remember();
        genie.leave();
        setupOwner = undefined;
      }
      void navigate({ tab: button.dataset.panelTab as PanelTab });
    };
  app.querySelector<HTMLButtonElement>("[data-queue-work]")!.onclick = () =>
    void navigate({ tab: "running" });
  app.querySelector<HTMLButtonElement>("[data-panel-status]")!.onclick = () =>
    void navigate({ tab: route.tab, detail: { type: "status" } });
  app.querySelector<HTMLButtonElement>("[data-panel-diagnostics]")!.onclick =
    () => void navigate({ tab: route.tab, detail: { type: "diagnostics" } });
  async function hide() {
    remember();
    try {
      await invoke("hide_panel");
    } catch (cause) {
      showError(
        typeof cause === "string" ? cause : "The panel could not be hidden.",
      );
    }
  }
  app.querySelector<HTMLButtonElement>("[data-panel-hide]")!.onclick = () =>
    void hide();
  document.addEventListener(
    "keydown",
    (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopImmediatePropagation();
        void hide();
      }
    },
    true,
  );
  window.addEventListener("blur", () => remember());
  window.addEventListener("focus", () => restorePosition(true));
  try {
    await listen<PanelSnapshot>(
      "pr-sniper:panel",
      (event) => void apply(event.payload),
      { target: "panel" },
    );
    const native = await invoke<PanelSnapshot>("panel_snapshot");
    await apply(native);
    const owner = intent;
    const current = () => intent === owner && revision === native.revision;
    try {
      const [initialSetup, initialWork] = await Promise.all([
        invoke<SetupReview>("monitoring_setup_review"),
        invoke<MonitoringSnapshot>("monitoring_snapshot"),
      ]);
      if (!current()) return;
      snapshot = initialWork;
      setupState(initialSetup);
      if (
        owner === 0 &&
        route.tab === "queue" &&
        !route.detail &&
        needsSetup(initialSetup, initialWork)
      ) {
        setupOwner = "queue";
        await apply(native);
        await genie.open("welcome");
      }
    } catch (cause) {
      if (current())
        showError(
          typeof cause === "string"
            ? cause
            : "Saved setup is unavailable. Open Settings to retry; no configuration was assumed.",
        );
    }
  } catch (cause) {
    showError(
      typeof cause === "string"
        ? cause
        : "Native panel routing is unavailable; no destination was substituted.",
    );
  }
}
