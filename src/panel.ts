import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mountSettings } from "./settings";
import { renderMonitoring, type MonitoringSnapshot } from "./monitoring";
import type { AutomationSnapshot } from "./automation";
import crosshair from "./crosshair.svg";
import sniperArt from "./sniper-mark.png";
import { humanQueue } from "./queue";
import {
  workPresentation,
  purposes,
  type WorkPresentation,
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
  app.innerHTML = `<header class="panel-header"><img src="${crosshair}" alt="" /><strong>PR Sniper</strong><div data-header-automation></div><button type="button" data-panel-hide aria-label="Hide PR Sniper panel" title="Close hides only; background work continues">${icon("m7 7 10 10M7 17 17 7")}</button></header>
    <div class="panel-context"><button type="button" data-panel-back aria-label="Back" hidden>${icon("m14 6-6 6 6 6M8 12h12")}</button><h1 tabindex="-1" data-panel-heading>Your queue</h1><img class="panel-art" src="${sniperArt}" alt="" /></div>
    <div class="panel-summary" data-panel-summary><strong data-summary-main>Reading queue...</strong><span data-summary-detail></span></div>
    <p class="panel-error" role="alert" data-panel-error hidden></p>
    <div class="panel-content">
      <div data-panel-view="monitor"></div>
      <section data-panel-view="running" hidden><div data-running-list></div></section>
      <section data-panel-view="reviewed" hidden></section>
      <div data-panel-view="settings" hidden></div>
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
    <footer class="panel-footer"><button type="button" data-panel-status>Status</button><button type="button" data-panel-diagnostics>Diagnostics</button><span>Close hides only. Quit from the tray menu.</span></footer>`;
  const error = app.querySelector<HTMLElement>("[data-panel-error]")!;
  const heading = app.querySelector<HTMLElement>("[data-panel-heading]")!;
  const content = app.querySelector<HTMLElement>(".panel-content")!;
  const back = app.querySelector<HTMLButtonElement>("[data-panel-back]")!;
  const views = {
    monitor: app.querySelector<HTMLElement>('[data-panel-view="monitor"]')!,
    running: app.querySelector<HTMLElement>('[data-panel-view="running"]')!,
    reviewed: app.querySelector<HTMLElement>('[data-panel-view="reviewed"]')!,
    settings: app.querySelector<HTMLElement>('[data-panel-view="settings"]')!,
    utility: app.querySelector<HTMLElement>('[data-panel-view="utility"]')!,
  };
  let route: PanelRoute = { tab: "queue" };
  let revision = -1;
  let returnTo: PanelRoute = { tab: "queue" };
  let settingsMounted = false;
  let snapshot: MonitoringSnapshot | undefined;
  let automation: AutomationSnapshot | undefined;
  let navigating = Promise.resolve();
  let utilityRevision = 0;
  let lastVisible = false;
  const positions = new Map<string, Position>();
  const listSignatures = new Map<PanelTab, string>();
  const key = (value: PanelRoute) => JSON.stringify(value);
  const showError = (message: string) => {
    error.textContent = message;
    error.hidden = false;
  };
  const remember = () => {
    const previous = positions.get(key(route));
    const focus =
      document.activeElement instanceof HTMLElement &&
      content.contains(document.activeElement)
        ? document.activeElement
        : (previous?.focus ?? null);
    const row = focus?.closest<HTMLElement>("[data-item-id],[data-job-id]");
    const nested = [...content.querySelectorAll<HTMLElement>("*")]
      .filter(
        (element) =>
          element.clientHeight > 0 &&
          element.scrollHeight > element.clientHeight &&
          ["auto", "scroll"].includes(getComputedStyle(element).overflowY),
      )
      .map((element): [HTMLElement, number] => [element, element.scrollTop]);
    positions.set(key(route), {
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
  content.addEventListener("focusin", remember);
  function restorePosition(focus: boolean) {
    const saved = positions.get(key(route));
    content.scrollTop = saved?.scroll ?? 0;
    for (const [element, scroll] of saved?.nested ?? [])
      if (element.isConnected && !element.closest("[hidden]"))
        element.scrollTop = scroll;
    if (!focus) return;
    const target = saved?.focus;
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
    navigate: (detail) =>
      void navigate({
        tab: route.tab === "settings" ? "queue" : route.tab,
        detail,
      }),
    onSnapshot: (value) => {
      snapshot = value;
      drawSummary();
      drawLists();
    },
    onAutomation: (value) => {
      automation = value;
      app.querySelector<HTMLElement>("[data-running-count]")!.textContent =
        value ? String(value.active) : "?";
      app.querySelector<HTMLButtonElement>(
        '[data-panel-tab="running"]',
      )!.title = value
        ? `${value.active} occupied AI slots, including ${value.stopping} stopping`
        : "Active work unavailable";
      drawSummary();
      drawLists();
    },
  });
  monitor.automation(
    app.querySelector<HTMLElement>("[data-header-automation]")!,
  );

  function drawSummary() {
    const summary = app.querySelector<HTMLElement>("[data-panel-summary]")!;
    summary.hidden =
      !!route.detail || ["settings", "reviewed"].includes(route.tab);
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
      const ready = items.filter(
        (item) => item.state === "machine_signed_off",
      ).length;
      main.textContent = snapshot
        ? `${items.length} for you`
        : "Queue unavailable";
      detail.textContent = snapshot
        ? `${ready} ready / ${items.length - ready} need attention`
        : "";
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
        void navigate({
          tab,
          detail:
            row.kind === "item"
              ? { type: "item", item_id: row.id }
              : { type: "job", kind: row.kind, id: row.id },
        });
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
      }>("snapshot");
      if (request !== utilityRevision) return;
      if (state.error) showError(state.error);
      const info = document.createElement("p");
      info.textContent = `PR Sniper ${state.version}. ${state.isolated ? "Isolated profile." : "Native host."} Hiding the panel does not stop background work.`;
      views.utility.append(info);
      const refresh = document.createElement("button");
      refresh.textContent = `Refresh ${type}`;
      refresh.onclick = () => void utility(type);
      views.utility.append(refresh);
      if (type === "diagnostics") {
        const entries =
          await invoke<{ timestamp_secs: number; event: string }[]>(
            "diagnostics",
          );
        if (request !== utilityRevision) return;
        const pre = document.createElement("pre");
        pre.textContent = entries.length
          ? entries
              .map(
                (e) =>
                  `${new Date(e.timestamp_secs * 1000).toISOString()}  ${e.event}`,
              )
              .join("\n")
          : "No host events recorded.";
        views.utility.append(pre);
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
    if (changed || !state.visible) remember();
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
    back.hidden = !route.detail;
    heading.textContent = route.detail
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
    const visible = utilityDetail
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
    if (visible === "settings" && !settingsMounted) {
      settingsMounted = true;
      await mountSettings(views.settings, { embedded: true });
      if (state.revision < revision) return;
    }
    if (visible === "monitor") monitor.detail(route.detail, state.missing);
    if (utilityDetail && (changed || !views.utility.children.length))
      void utility(route.detail!.type as "status" | "diagnostics");
    if (changed || (state.visible && !lastVisible))
      requestAnimationFrame(() => restorePosition(true));
    lastVisible = state.visible;
  }
  function navigate(next: PanelRoute) {
    navigating = navigating.then(async () => {
      remember();
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
      }
    });
    return navigating;
  }
  back.onclick = () =>
    void navigate(returnTo.detail ? { tab: route.tab } : returnTo);
  for (const button of app.querySelectorAll<HTMLButtonElement>(
    "[data-panel-tab]",
  ))
    button.onclick = () =>
      void navigate({ tab: button.dataset.panelTab as PanelTab });
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
  window.addEventListener("blur", remember);
  window.addEventListener("focus", () => restorePosition(true));
  try {
    await listen<PanelSnapshot>(
      "pr-sniper:panel",
      (event) => void apply(event.payload),
      { target: "panel" },
    );
    await apply(await invoke<PanelSnapshot>("panel_snapshot"));
  } catch (cause) {
    showError(
      typeof cause === "string"
        ? cause
        : "Native panel routing is unavailable; no destination was substituted.",
    );
  }
}
