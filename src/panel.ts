import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mountSettings } from "./settings";
import { mountGenie } from "./genie";
import { type SetupReview } from "./resources";
import { renderMonitoring, type MonitoringSnapshot } from "./monitoring";
import type { AutomationSnapshot, FailureClass } from "./automation";
import crosshair from "./crosshair.svg";
import sniperArt from "./sniper-mark.png";
import { humanQueue, teammateChip } from "./queue";
import { retryCommands, runRecoveries } from "./queue-detail";
import {
  workPresentation,
  purposes,
  type WorkPresentation,
} from "./work-presentation";
import changelogSource from "../CHANGELOG.md?raw";
import "./panel.css";
import { terseError } from "./errors";
import { inlineMarkdown } from "./markdown";
import { drawNow, setText, whenReleased } from "./stable-dom";

export type PanelTab = "queue" | "running" | "reviewed" | "settings";
export type WorkKind = "normal" | "reply" | "mention" | "primary_final";
export type PanelDetail =
  | { type: "item"; item_id: string }
  | { type: "job"; kind: WorkKind; id: string };
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
  failure?: FailureClass | null;
  delayed_until?: number | null;
  teammate?: string | null;
  work?: WorkPresentation;
  subject?: string;
  reference?: string;
}

const stateIcons: Record<string, string> = {
  Queued: "M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  Waiting: "M12 7v5l3 2M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  Failed: "m9 9 6 6m0-6-6 6M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  Credentials: "m9 9 6 6m0-6-6 6M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  Permission: "m9 9 6 6m0-6-6 6M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  Network: "m9 9 6 6m0-6-6 6M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  Stopping: "M9 9v6m6-6v6M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18",
  Superseded: "M7 7h10M7 12h10M7 17h6",
};

const cleanIcon = "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v5M14 11v5";

const genieIcon =
  "M10.2 6.4V4.2a1.8 1.8 0 0 1 3.6 0v2.2M9.6 7.4c1.4 1 3.4 1 4.8 0M12 8.2C9 7.4 3 8.4 3 11.4c0 2.6 4.5 3.4 7.4 1.6M12 8.2c3-.8 9 .2 9 3.2 0 2.6-4.5 3.4-7.4 1.6M10.4 13c1.2.5 2.8.5 3.2 0M7.6 14.2c.6 3.4 2.8 5.7 5.8 6.6 2.2.4 4-.3 5-1.6-1.5.5-2.9.1-3.7-1 1.2-1.5 1.7-3.1 1.6-5";

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
const REVIEWED_PAGE = 50;

export async function mountPanel(app: HTMLElement) {
  app.className = "panel-shell";
  app.innerHTML = `<header class="panel-header"><img src="${crosshair}" alt="" /><strong>PR Sniper</strong><span data-setup-needed hidden>Setup needed</span><div data-header-automation></div><button type="button" data-panel-hide aria-label="Hide PR Sniper panel" title="Hide">${icon("m7 7 10 10M7 17 17 7")}</button></header>
    <div class="panel-context"><button type="button" data-panel-back aria-label="Back" hidden>${icon("m14 6-6 6 6 6M8 12h12")}</button><h1 tabindex="-1" data-panel-heading>Your queue</h1><button type="button" class="panel-genie" data-context-genie aria-label="Set up with Genie" title="Set up with Genie">${icon(genieIcon)}</button><img class="panel-art" src="${sniperArt}" alt="" /></div>
    <div class="panel-summary" data-panel-summary>
      <div data-work-summary><strong data-summary-main>Loading</strong></div>
      <div class="queue-metrics" role="group" aria-label="Human handoffs" data-queue-metrics hidden>
        <button type="button" class="queue-metric" data-queue-ready-card data-queue-filter="ready" aria-pressed="false"><span class="queue-metric-label">Ready for you${icon(destinations.queue.icon)}</span><span class="queue-metric-value"><strong data-queue-ready>?</strong></span></button>
        <button type="button" class="queue-metric" data-queue-attention-card data-queue-filter="attention" aria-pressed="false"><span class="queue-metric-label">Needs attention${icon("M12 8v5m0 3h.01M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18")}</span><span class="queue-metric-value"><strong data-queue-attention>?</strong></span></button>
      </div>
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
      <section data-panel-view="changelog" class="changelog" hidden></section>
    </div>
    <nav class="panel-tabs" data-panel-navigation aria-label="Application destinations">${(
      ["queue", "running", "reviewed", "settings"] as const
    )
      .map(
        (tab) =>
          `<button type="button" data-panel-tab="${tab}" aria-label="${tab[0].toUpperCase() + tab.slice(1)}"><span class="nav-icon">${icon(destinations[tab].icon)}${tab === "running" ? '<span data-running-count aria-hidden="true">?</span>' : ""}</span><span>${tab[0].toUpperCase() + tab.slice(1)}</span></button>`,
      )
      .join("")}</nav>
    <footer class="panel-footer"><button type="button" data-panel-version aria-label="Show changelog">Reading version...</button></footer>`;
  const version = app.querySelector<HTMLElement>("[data-panel-version]")!;
  let versionKnown = false;
  const showVersion = (value: unknown, failure?: unknown) => {
    if (typeof value === "string" && value.trim()) {
      versionKnown = true;
      version.textContent = `v${value.trim()}`;
      version.title = `Running PR Sniper ${value.trim()}`;
    } else if (!versionKnown) {
      version.textContent = "Version unavailable";
      version.title =
        failure === undefined
          ? "Unavailable"
          : `Unavailable: ${String(failure)}`;
    }
  };
  const readVersion = () =>
    void invoke<{ version: unknown }>("snapshot").then(
      (state) => showVersion(state.version),
      (cause) => showVersion(undefined, cause),
    );
  readVersion();
  const error = app.querySelector<HTMLElement>("[data-panel-error]")!;
  const heading = app.querySelector<HTMLElement>("[data-panel-heading]")!;
  const content = app.querySelector<HTMLElement>(".panel-content")!;
  const context = app.querySelector<HTMLElement>(".panel-context")!;
  // Cards overlap the title band at rest; scrolled content slides under the band
  // instead of drawing over it. Covers every scroller inside the content area.
  const clipUnderBand = (scroller: HTMLElement) => {
    // A band that scrolls with the content never needs a clip.
    const overlap = scroller.contains(context)
      ? 0
      : context.getBoundingClientRect().bottom -
        scroller.getBoundingClientRect().top;
    const clip = Math.max(0, Math.min(scroller.scrollTop, overlap));
    scroller.style.clipPath = clip > 0 ? `inset(${clip}px 0 0 0)` : "";
  };
  content.addEventListener(
    "scroll",
    (event) => {
      if (event.target instanceof HTMLElement) clipUnderBand(event.target);
    },
    { capture: true, passive: true },
  );
  const summary = app.querySelector<HTMLElement>("[data-panel-summary]")!;
  const back = app.querySelector<HTMLButtonElement>("[data-panel-back]")!;
  const contextGenie = app.querySelector<HTMLButtonElement>(
    "[data-context-genie]",
  )!;
  contextGenie.onclick = () => void showSetup("settings", contextGenie);
  const views = {
    monitor: app.querySelector<HTMLElement>('[data-panel-view="monitor"]')!,
    running: app.querySelector<HTMLElement>('[data-panel-view="running"]')!,
    reviewed: app.querySelector<HTMLElement>('[data-panel-view="reviewed"]')!,
    settings: app.querySelector<HTMLElement>('[data-panel-view="settings"]')!,
    genie: app.querySelector<HTMLElement>('[data-panel-view="genie"]')!,
    changelog: app.querySelector<HTMLElement>('[data-panel-view="changelog"]')!,
  };
  let changelogOpen = false;
  let lastState: PanelSnapshot | undefined;
  function renderChangelog() {
    const view = views.changelog;
    view.replaceChildren();
    let list: HTMLUListElement | undefined;
    let item: { element: HTMLLIElement; text: string } | undefined;
    const flush = () => {
      if (item) inlineMarkdown(item.element, item.text);
      item = undefined;
    };
    for (const raw of changelogSource.split("\n").slice(1)) {
      const line = raw.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1");
      const heading = /^(##|###) (.+)$/.exec(line);
      if (heading) {
        flush();
        list = undefined;
        const h = document.createElement(heading[1] === "##" ? "h2" : "h3");
        inlineMarkdown(h, heading[2]);
        view.append(h);
      } else if (line.startsWith("- ")) {
        flush();
        list ??= view.appendChild(document.createElement("ul"));
        item = {
          element: list.appendChild(document.createElement("li")),
          text: line.slice(2),
        };
      } else if (/^\s+\S/.test(line) && item) {
        item.text += ` ${line.trim()}`;
      }
    }
    flush();
    view.scrollTop = 0;
  }
  function setChangelog(open: boolean) {
    changelogOpen = open;
    if (open) renderChangelog();
    if (lastState) void apply(lastState);
  }
  version.onclick = () => {
    if (!versionKnown) readVersion();
    setChangelog(!changelogOpen);
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
  let lastVisible = false;
  let focusRevision = 0;
  let setupRead = 0;
  const positions = new Map<string, Position>();
  // Reviewed rows drawn; grows by a page per Show more, reset when the tab reopens.
  let reviewedLimit = REVIEWED_PAGE;
  // Tab of the latest navigation request; the route updates only after it resolves.
  let tabIntent: PanelTab = "queue";
  const listSignatures = new Map<PanelTab, string>();
  const key = (value: PanelRoute) => JSON.stringify(value);
  const positionKey = () =>
    setupVisible() ? `${key(route)}:genie` : key(route);
  const setupVisible = () => setupOwner === route.tab && !route.detail;
  const showError = (message: string) => {
    const label = terseError(message);
    if (label === null) return;
    error.textContent = label;
    // The backend sentence stays available to assistive tech and hover.
    error.title = message;
    error.setAttribute("aria-description", message);
    error.hidden = false;
  };
  error.onclick = () => {
    error.hidden = true;
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
    fix: (target) => {
      const origin = route;
      const task = navigate({ tab: "settings" });
      const expected = intent;
      void task.then(async () => {
        const controller = await ensureSettings();
        if (intent !== expected || route.tab !== "settings" || route.detail)
          return;
        controller.open(target, { back: () => void navigate(origin) });
      });
    },
    onSnapshot: (value) => {
      snapshot = value;
      drawSummary();
      drawLists();
    },
    onAutomation: (value) => {
      automation = value;
      app.dataset.automationUnavailable = String(!value);
      const runningCount = app.querySelector<HTMLElement>(
        "[data-running-count]",
      )!;
      setText(runningCount, value ? String(value.active) : "?");
      // A zero badge is noise; the count stays in the DOM for state checks.
      runningCount.dataset.zero = String(value?.active === 0);
      app.querySelector<HTMLButtonElement>(
        '[data-panel-tab="running"]',
      )!.title = value
        ? `${value.active} occupied AI slots, including ${value.stopping} stopping`
        : "Unavailable";
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
          showError(typeof cause === "string" ? cause : "Unavailable");
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
  let settingsSub: { title: string; back: boolean } | undefined;
  let settingsEditor: { title: string; back: () => void } | undefined;
  const subSection = () =>
    route.tab === "settings" && !route.detail && !setupVisible()
      ? (settingsEditor ??
        (settingsSub?.back && settingsSub.title ? settingsSub : undefined))
      : undefined;
  const syncSub = () => {
    const sub = subSection();
    app.dataset.settingsSub = String(!!sub);
    app.dataset.settingsEditor = String(!!settingsEditor && !!sub);
    heading.textContent = sub ? sub.title : destinations[route.tab].title;
    back.hidden =
      !sub && !route.detail && (!setupVisible() || setupTitle === "Welcome");
  };
  views.settings.addEventListener("pr-sniper:settings-section", (event) => {
    settingsSub = (event as CustomEvent).detail;
    syncSub();
  });
  views.settings.addEventListener("pr-sniper:settings-editor", (event) => {
    settingsEditor = (event as CustomEvent).detail ?? undefined;
    syncSub();
  });
  const ensureSettings = () =>
    (settingsController ??= mountSettings(views.settings, {
      embedded: true,
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
        if (paused && intent === expected) showError("Confirmed; paused");
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
  app.addEventListener("pr-sniper:retry-items", (event) => {
    const ids = (event as CustomEvent<{ ids: string[] }>).detail.ids;
    void (async () => {
      // Several PRs can share one recovery (a repository's poll retry); poll
      // retries go first, then every failed review is enqueued.
      const commands = ids.flatMap((id) => {
        const item = snapshot?.items?.find((entry) => entry.id === id);
        return item && snapshot ? retryCommands(item, snapshot) : [];
      });
      commands.sort(
        (a, b) =>
          Number(a[0] === "start_review") - Number(b[0] === "start_review"),
      );
      await runRecoveries(commands, (error) =>
        showError(typeof error === "string" ? error : "Failed"),
      );
      void monitor.refresh();
    })();
  });
  app.addEventListener("pr-sniper:fix-account", (event) => {
    remember(event.target instanceof HTMLElement ? event.target : undefined);
    const task = navigate({ tab: "settings" });
    const expected = intent;
    void task.then(async () => {
      const controller = await ensureSettings();
      if (intent !== expected || route.tab !== "settings" || route.detail)
        return;
      controller.open("accounts", {
        back: () => void navigate({ tab: "queue" }),
      });
    });
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

  let queueFilter: "ready" | "attention" | undefined;
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
      changelogOpen ||
      setupVisible() ||
      !!route.detail ||
      ["settings", "reviewed", "running"].includes(route.tab);
    const main = summary.querySelector<HTMLElement>("[data-summary-main]")!;
    const detail = summary.querySelector<HTMLElement>("[data-summary-detail]")!;
    if (route.tab === "running") {
      setText(
        main,
        automation
          ? `${automation.active - automation.stopping} / ${automation.capacity} running`
          : "Unavailable",
      );
      setText(
        detail,
        automation
          ? `${waitingCount()} waiting${automation.stopping ? ` / ${automation.stopping} stopping` : ""}`
          : "Occupancy unknown",
      );
    } else {
      const items = humanQueue(snapshot?.items ?? []);
      const known = Array.isArray(snapshot?.items);
      const ready = items.filter(
        (item) => item.state === "machine_signed_off",
      ).length;
      setText(main, known ? `${items.length} for you` : "Queue unavailable");
      setText(
        detail,
        known
          ? `${ready} ready / ${items.length - ready} need attention`
          : "Handoffs unavailable",
      );
      const view = queueFilter ?? (ready > 0 ? "ready" : "attention");
      app.dataset.queueView = view;
      for (const button of summary.querySelectorAll<HTMLElement>(
        "[data-queue-filter]",
      ))
        button.setAttribute(
          "aria-pressed",
          String(button.dataset.queueFilter === view),
        );
      setText(
        summary.querySelector<HTMLElement>("[data-queue-ready]")!,
        known ? String(ready) : "?",
      );
      setText(
        summary.querySelector<HTMLElement>("[data-queue-attention]")!,
        known ? String(items.length - ready) : "?",
      );
      setText(
        summary.querySelector<HTMLElement>("[data-queue-capacity]")!,
        automation ? `${automation.active}/${automation.capacity}` : "?",
      );
      setText(
        summary.querySelector<HTMLElement>("[data-queue-work-status]")!,
        automation
          ? `${automation.active - automation.stopping} running${automation.stopping ? ` / ${automation.stopping} stopping` : ""} / ${waitingCount()} waiting${automation.paused ? " / paused" : ""}`
          : "Unavailable",
      );
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
  // Lists redraw after a press ends so a refresh never drops its click.
  const pendingRows = new Map<"running" | "reviewed", Row[]>();
  const drawers = {
    running: () => render("running", pendingRows.get("running") ?? []),
    reviewed: () => render("reviewed", pendingRows.get("reviewed") ?? []),
  };
  // `now`: user-initiated redraws (Show more, opening a tab) never wait.
  function draw(tab: "running" | "reviewed", rows: Row[], now = false) {
    pendingRows.set(tab, rows);
    if (now) drawNow(drawers[tab]);
    else whenReleased(drawers[tab]);
  }
  function render(tab: "running" | "reviewed", rows: Row[]) {
    const limit = tab === "reviewed" ? reviewedLimit : rows.length;
    const signature = JSON.stringify([rows, limit]);
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
    const list = document.createElement("ol");
    list.className = "work-list";
    list.setAttribute(
      "aria-label",
      tab === "running" ? "Agent work queue" : "Reviewed work",
    );
    if (rows.length) root.append(list);
    const terminal = (row: Row) =>
      row.kind === "item" && ["closed", "merged"].includes(row.state);
    const clean = async (itemId?: string) => {
      try {
        await invoke("clean_reviewed", { itemId: itemId ?? null });
      } catch (cause) {
        showError(typeof cause === "string" ? cause : "Unavailable");
      }
      void monitor.refresh();
    };
    const terminalCount = tab === "reviewed" ? rows.filter(terminal).length : 0;
    const shown = rows.slice(0, limit);
    const hidden = rows.length - shown.length;
    if (hidden > 0) {
      const more = document.createElement("button");
      more.type = "button";
      more.className = "work-more";
      more.dataset.showMore = "";
      more.setAttribute(
        "aria-label",
        `Show ${Math.min(hidden, REVIEWED_PAGE)} more`,
      );
      more.innerHTML = `${icon("m6 9 6 6 6-6")}<span>+${hidden}</span>`;
      more.onclick = () => {
        const first = limit;
        reviewedLimit = limit + REVIEWED_PAGE;
        drawLists(true);
        const target =
          root.querySelectorAll<HTMLButtonElement>(".work-row")[first];
        if (!target) return;
        target.focus();
        const saved = positions.get(key(route));
        if (saved) saved.focus = target;
      };
      root.append(more);
    }
    if (terminalCount) {
      const all = document.createElement("button");
      all.type = "button";
      all.className = "work-retry-all work-clean-all";
      all.setAttribute("aria-label", "Clean all");
      all.title = "Clean all";
      all.innerHTML = `${icon(cleanIcon)}<span>${terminalCount}</span>`;
      all.onclick = () => {
        all.disabled = true;
        void clean();
      };
      root.append(all);
    }
    if (!rows.length) {
      const empty = document.createElement("div");
      empty.className = "queue-empty";
      empty.innerHTML = `${icon(destinations[tab].icon)}<span class="sr-only">${tab === "running" ? "No jobs" : "None"}</span>`;
      root.append(empty);
    }
    for (const row of shown) {
      const article = document.createElement("article");
      article.dataset.jobId = `${row.kind}:${row.id}`;
      article.dataset.workState = row.state;
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
      {
        const displayState =
          tab === "reviewed"
            ? row.kind === "item"
              ? row.state[0].toUpperCase() + row.state.slice(1)
              : "Reviewed"
            : row.state === "active"
              ? "Running"
              : row.state === "stopping"
                ? "Stopping"
                : row.work?.state === "superseded"
                  ? "Superseded"
                  : ["failed", "manual_retry"].includes(row.work?.state ?? "")
                    ? "Failed"
                    : row.delayed_until
                      ? "Waiting"
                      : "Queued";
        button.className = "work-row";
        button.dataset.display = displayState.toLowerCase();
        button.setAttribute(
          "aria-label",
          row.kind === "item" ? "Open PR evidence" : "Open job",
        );
        button.title = `${row.title}; ${displayState}`;
        button.replaceChildren();
        const marker = document.createElement("span");
        marker.className =
          row.state === "active" && tab === "running"
            ? "work-spin"
            : "work-marker";
        marker.setAttribute("aria-hidden", "true");
        if (row.state !== "active" || tab === "reviewed")
          marker.innerHTML = icon(
            tab === "reviewed"
              ? destinations.reviewed.icon
              : (stateIcons[displayState] ?? destinations.running.icon),
          );
        const copy = document.createElement("span");
        copy.className = "work-copy";
        const state = document.createElement("span");
        state.className = "work-state";
        // Backpressure is a quiet wait: a clock and the local resume time.
        state.textContent = row.delayed_until
          ? new Date(row.delayed_until * 1000).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })
          : displayState;
        if (row.failure) {
          const subject = document.createElement("strong");
          subject.className = "work-subject";
          subject.textContent = row.work?.subject ?? row.subject ?? row.title;
          const reference = document.createElement("span");
          reference.className = "work-reference";
          reference.textContent = row.work?.reference ?? row.reference ?? "";
          copy.append(subject, reference);
        } else {
          const top = document.createElement("span");
          top.className = "work-heading";
          const agent = document.createElement("strong");
          agent.textContent =
            row.work?.agent ??
            (row.kind === "item" ? row.title : "Job evidence unavailable");
          top.append(agent, state);
          const subject = document.createElement("span");
          subject.className = "work-subject";
          subject.textContent =
            row.work?.subject ?? (row.kind === "item" ? "" : row.title);
          const reference = document.createElement("span");
          reference.className = "work-reference";
          reference.textContent =
            row.work?.reference ??
            (row.kind === "item" ? "" : "Repository unavailable");
          const purpose = document.createElement("span");
          purpose.textContent =
            row.work?.ordinal ??
            (row.kind === "item" ? "PR" : purposes[row.kind]);
          reference.append(purpose);
          copy.append(top, subject, reference);
          if (row.teammate && tab === "running")
            copy.append(teammateChip(row.teammate, null));
        }
        if (row.reason) button.title += `. ${row.reason}`;
        const chevron = document.createElement("span");
        chevron.className = "work-chevron";
        chevron.setAttribute("aria-hidden", "true");
        if (row.failure) button.append(marker, copy, state, chevron);
        else button.append(marker, copy, chevron);
        article.append(button);
        if (terminal(row)) {
          const remove = document.createElement("button");
          remove.type = "button";
          remove.className = "work-clean";
          remove.dataset.cleanItem = row.id;
          remove.setAttribute("aria-label", "Clean");
          remove.title = "Clean";
          remove.innerHTML = icon(cleanIcon);
          remove.onclick = () => {
            remove.disabled = true;
            void clean(row.id);
          };
          article.append(remove);
        }
        const entry = document.createElement("li");
        entry.append(article);
        list.append(entry);
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
  // Waiting work excludes parked failures, which belong to Queue.
  function waitingCount() {
    return (automation?.work ?? []).filter(
      (work) =>
        ["waiting", "blocked"].includes(work.state) &&
        !work.failure &&
        !["failed", "manual_retry"].includes(
          (snapshot &&
            workPresentation(snapshot, work.key.kind, work.key.id)?.state) ??
            "",
        ),
    ).length;
  }
  function drawLists(now = false) {
    if (!automation) {
      views.running.querySelector<HTMLElement>(
        "[data-running-list]",
      )!.textContent = "Unavailable";
      listSignatures.delete("running");
    } else {
      const rows: Row[] = [...automation.work]
        .sort((a, b) => {
          const rank = (state: string) =>
            state === "active" ? 0 : state === "stopping" ? 1 : 2;
          return (
            rank(a.state) - rank(b.state) || a.enqueue_order - b.enqueue_order
          );
        })
        .map((work) => {
          const presentation =
            snapshot && workPresentation(snapshot, work.key.kind, work.key.id);
          return {
            id: work.key.id,
            kind: work.key.kind,
            title:
              metadata(work.key.kind, work.key.id)?.title ??
              `Saved ${work.key.kind} job ${work.key.id}`,
            state: work.state,
            failure: work.failure,
            delayed_until: work.delayed_until,
            teammate: work.blocked_by?.teammate.agent_name ?? null,
            reason:
              work.reason ??
              (automation?.paused && work.state === "waiting"
                ? "Monitoring paused."
                : null),
            work: presentation,
          };
        });
      // Parked failures belong to Queue > Needs attention, never here.
      const running = rows.filter(
        (row) =>
          row.work?.state === "superseded" ||
          (!row.failure &&
            !["failed", "manual_retry"].includes(row.work?.state ?? "")),
      );
      rows.splice(0, rows.length, ...running);
      draw("running", rows, now);
    }
    if (!snapshot) {
      views.reviewed.textContent = "Unavailable";
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
          work: workPresentation(snapshot, "normal", review.key),
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
            work: workPresentation(snapshot, kind, follow.run.id),
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
          work: workPresentation(snapshot, "primary_final", final.id),
        });
      if (["approved", "closed", "merged"].includes(item.state)) {
        rows.push({
          id: item.id,
          kind: "item",
          title: `${item.job.repository_name} #${item.job.number}: ${item.job.title}`,
          state: item.state,
        });
      }
    }
    draw("reviewed", rows, now);
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
    if (route.tab === "reviewed" && tabIntent !== "reviewed")
      reviewedLimit = REVIEWED_PAGE;
    tabIntent = route.tab;
    revision = state.revision;
    lastState = state;
    if (changed) changelogOpen = false;
    app.dataset.nativeVisible = String(state.visible);
    if (state.placement_warning) showError(state.placement_warning);
    const sub = subSection();
    back.hidden =
      !changelogOpen &&
      !route.detail &&
      !sub &&
      (!setupVisible() || setupTitle === "Welcome");
    heading.textContent = changelogOpen
      ? "Changelog"
      : setupVisible()
        ? setupTitle
        : sub
          ? sub.title
          : route.detail
            ? route.detail.type === "job"
              ? "Job details"
              : "Saved evidence"
            : destinations[route.tab].title;
    app.dataset.tab = route.tab;
    app.dataset.changelog = String(changelogOpen);
    app.dataset.settingsSub = String(!!sub);
    app.dataset.settingsEditor = String(!!settingsEditor && !!sub);
    app.dataset.detail = String(!!route.detail);
    app.dataset.evidence = route.detail?.type ?? "";
    app.dataset.setup = String(setupVisible());
    const scrollQueue =
      route.tab === "queue" && !setupVisible() && !changelogOpen;
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
    // The Queue list keeps its title pinned; detail views share the scroll budget.
    // Only re-parent when needed: moving a node drops focus from the heading.
    const inContent = (node: Element) => node.parentElement === content;
    if (scrollQueue && route.detail) {
      if (!inContent(context) || !inContent(summary))
        content.prepend(context, summary, error);
    } else if (scrollQueue) {
      if (inContent(context)) content.before(context);
      if (!inContent(summary)) content.prepend(summary, error);
    } else if (inContent(context) || inContent(summary)) {
      content.before(context, summary, error);
    }
    clipUnderBand(content);
    drawSummary();
    for (const button of app.querySelectorAll<HTMLButtonElement>(
      "[data-panel-tab]",
    )) {
      if (button.dataset.panelTab === route.tab)
        button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    }
    const visible = changelogOpen
      ? "changelog"
      : setupVisible()
        ? "genie"
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
    if (next.tab === "reviewed" && tabIntent !== "reviewed") {
      reviewedLimit = REVIEWED_PAGE;
      drawLists(true);
    }
    tabIntent = next.tab;
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
        showError(typeof cause === "string" ? cause : "Failed");
      } finally {
        navigationOrigin = undefined;
      }
    });
    return navigating;
  }
  back.onclick = () => {
    if (changelogOpen) setChangelog(false);
    else if (settingsEditor && subSection()) settingsEditor.back();
    else if (subSection())
      app.querySelector<HTMLElement>(".settings-home-back")!.click();
    else if (setupVisible()) void genie.back();
    else void navigate(returnTo.detail ? { tab: route.tab } : returnTo);
  };
  for (const button of app.querySelectorAll<HTMLButtonElement>(
    "[data-panel-tab]",
  ))
    button.onclick = () => {
      changelogOpen = false;
      if (setupVisible()) {
        remember();
        genie.leave();
        setupOwner = undefined;
      }
      void navigate({ tab: button.dataset.panelTab as PanelTab });
    };
  for (const button of app.querySelectorAll<HTMLButtonElement>(
    "[data-queue-filter]",
  ))
    button.onclick = () => {
      queueFilter = button.dataset.queueFilter as "ready" | "attention";
      drawSummary();
    };
  app.querySelector<HTMLButtonElement>("[data-queue-work]")!.onclick = () =>
    void navigate({ tab: "running" });
  async function hide() {
    remember();
    try {
      await invoke("hide_panel");
    } catch (cause) {
      showError(typeof cause === "string" ? cause : "Failed");
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
        showError(typeof cause === "string" ? cause : "Unavailable");
    }
  } catch (cause) {
    showError(typeof cause === "string" ? cause : "Unavailable");
  }
}
