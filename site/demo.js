// Scripted, deterministic simulation of the PR Sniper tray panel. Every repo,
// person and pull request below is fictional.

const H = "─".repeat(240);
const V = "│\n".repeat(90);
const TICK = 250;
const STEP_MS = 2000;
const BANNER_MS = 5000;
const MAX_BANNERS = 3;
const START_SECONDS = 9 * 3600 + 41 * 60;

export function frame(el) {
  if (el.querySelector(":scope > .fb")) return;
  const parts = [
    ["fb fb-t", H],
    ["fb fb-b", H],
    ["fb fb-l", V],
    ["fb fb-r", V],
    ["fb fb-c fb-tl", "┌"],
    ["fb fb-c fb-tr", "┐"],
    ["fb fb-c fb-bl", "└"],
    ["fb fb-c fb-br", "┘"],
  ];
  for (const [className, text] of parts) {
    const strip = document.createElement("span");
    strip.className = className;
    strip.setAttribute("aria-hidden", "true");
    strip.textContent = text;
    el.append(strip);
  }
}

const AGENTS = {
  snake: "SOLID Snake",
  sentinel: "Sentinel",
};

const PRS = {
  p58: {
    repo: "quill/inkwell",
    num: 58,
    title: "Trim retry jitter on sync",
    author: "sasha-b",
  },
  p61: {
    repo: "quill/inkwell",
    num: 61,
    title: "Cache avatar lookups",
    author: "o.tanaka",
  },
  p142: {
    repo: "lumen/ledger",
    num: 142,
    title: "Fix token refresh race",
    author: "mira-k",
  },
  p138: {
    repo: "lumen/ledger",
    num: 138,
    title: "Docs: clarify cron syntax",
    author: "mira-k",
  },
  p317: {
    repo: "harbor/tidepool",
    num: 317,
    title: "Stream large CSV exports",
    author: "devraj",
  },
  p311: {
    repo: "harbor/tidepool",
    num: 311,
    title: "Bump parser to 3.2",
    author: "devraj",
  },
};

// Titles and first sentences come from src-tauri/src/notifications.rs.
const NOTICES = {
  ready: {
    title: "Ready for your final review",
    body: "Automated review completed.",
  },
  failure: {
    title: "PR Sniper needs attention",
    body: "A review or monitoring operation needs recovery.",
  },
  merged: { title: "PR merged on GitHub", body: "GitHub confirmed merge." },
};

const VIEWS = ["queue", "running", "reviewed"];
const VIEW_LABELS = {
  queue: "Queue",
  running: "Running",
  reviewed: "Reviewed",
};
const VIEW_TITLES = {
  queue: "Your queue",
  running: "Work queue",
  reviewed: "Reviewed",
};

function initialState() {
  return {
    ready: [{ pr: "p58", agent: "sentinel" }],
    attention: [],
    running: [],
    reviewed: [
      { pr: "p311", agent: "sentinel", label: "Merged" },
      { pr: "p138", agent: "snake", label: "Reviewed" },
    ],
  };
}

const ref = (key) => `${PRS[key].repo} #${PRS[key].num}`;
const job = (s, key) => s.running.find((item) => item.pr === key);
const drop = (list, key) => list.filter((item) => item.pr !== key);

const SCRIPT = [
  { at: 4, log: "tick lumen/ledger · Every 5 min · 1 new PR", view: "running" },
  {
    at: 6,
    log: "admitted lumen/ledger #142 · rule assigned · @mira-k",
    run: (s) => s.running.push({ pr: "p142", agent: "snake", state: "Queued" }),
    view: "running",
  },
  {
    at: 9,
    log: "SOLID Snake started #142 · head 3f9c2e1 · Voice Caveman",
    run: (s) => (job(s, "p142").state = "Running"),
    view: "running",
  },
  { at: 31, log: "SOLID Snake read 7 of 7 files", view: "running" },
  {
    at: 44,
    log: "Rubber Duck Mallard: confirmed · auth/refresh.ts:88",
    view: "running",
  },
  {
    at: 47,
    log: 'Publish Comment · 2 inline · "Two callers. One token. Add lock."',
    view: "running",
  },
  {
    at: 49,
    log: "Ready for you · lumen/ledger #142",
    run: (s) => {
      s.running = drop(s.running, "p142");
      s.ready.unshift({ pr: "p142", agent: "snake" });
    },
    view: "queue",
    card: "ready",
    notice: { kind: "ready", view: "queue", card: "ready", key: "ready:p142" },
  },
  {
    at: 62,
    log: "tick harbor/tidepool · Every 15 min · 1 new PR",
    view: "queue",
  },
  {
    at: 64,
    log: "admitted harbor/tidepool #317 · rule all · @devraj",
    run: (s) =>
      s.running.push({ pr: "p317", agent: "sentinel", state: "Queued" }),
    view: "running",
  },
  {
    at: 67,
    log: "Sentinel started #317 · head a1b2c3d · Voice Merlin",
    run: (s) => (job(s, "p317").state = "Running"),
    view: "running",
  },
  {
    at: 80,
    log: "push harbor/tidepool #317 · a1b2c3d → 9e8f7a6",
    view: "running",
  },
  {
    at: 82,
    log: "Superseded · #317 review of a1b2c3d · no attempt spent",
    run: (s) => (job(s, "p317").state = "Superseded"),
    view: "running",
  },
  {
    at: 95,
    log: "requeued #317 at 9e8f7a6 · Pass 1",
    run: (s) => (job(s, "p317").state = "Queued"),
    view: "running",
  },
  {
    at: 98,
    log: "tick quill/inkwell · Hourly at :15 · admitted #61 · @o.tanaka",
    run: (s) => s.running.push({ pr: "p61", agent: "snake", state: "Queued" }),
    view: "running",
  },
  {
    at: 101,
    log: "Sentinel started #317 · head 9e8f7a6",
    run: (s) => (job(s, "p317").state = "Running"),
    view: "running",
  },
  {
    at: 103,
    log: "SOLID Snake started #61 · head 5d4c3b2",
    run: (s) => (job(s, "p61").state = "Running"),
    view: "running",
  },
  {
    at: 131,
    log: "Ready for you · harbor/tidepool #317 · summary kept local",
    run: (s) => {
      s.running = drop(s.running, "p317");
      s.ready.unshift({ pr: "p317", agent: "sentinel" });
    },
    view: "queue",
    card: "ready",
    notice: { kind: "ready", view: "queue", card: "ready", key: "ready:p317" },
  },
  {
    at: 140,
    log: "#61 attempt 1 of 3 failed · Network · retry with backoff",
    run: (s) => (job(s, "p61").state = "Queued"),
    view: "running",
  },
  {
    at: 170,
    log: "#61 attempt 3 of 3 failed · Network · parked",
    run: (s) => {
      s.running = drop(s.running, "p61");
      s.attention.unshift({ pr: "p61", agent: "snake", cause: "Network" });
    },
    view: "queue",
    card: "attention",
    notice: {
      kind: "failure",
      view: "queue",
      card: "attention",
      key: "attention:p61",
    },
  },
  {
    at: 186,
    log: "merged on GitHub · quill/inkwell #58 · @sasha-b",
    run: (s) => {
      s.ready = drop(s.ready, "p58");
      s.reviewed.unshift({ pr: "p58", agent: "sentinel", label: "Merged" });
    },
    view: "reviewed",
    notice: { kind: "merged", view: "reviewed", key: "reviewed:p58" },
  },
  {
    at: 192,
    log: "end of script · restarting",
    view: "reviewed",
    restart: true,
  },
];

// Under reduced motion the demo opens paused on this step.
const MID_STEP = 12;

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};

const scopeIcon = () => {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 32 32");
  svg.setAttribute("aria-hidden", "true");
  const circle = document.createElementNS(ns, "circle");
  circle.setAttribute("cx", "16");
  circle.setAttribute("cy", "16");
  circle.setAttribute("r", "9");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", "M16 2v9m0 10v9M2 16h9m10 0h9");
  svg.append(circle, path);
  return svg;
};

const clock = (seconds, withDay) => {
  const h = Math.floor(seconds / 3600) % 24;
  const m = String(Math.floor(seconds / 60) % 60).padStart(2, "0");
  const sec = String(seconds % 60).padStart(2, "0");
  if (!withDay) return `${String(h).padStart(2, "0")}:${m}:${sec}`;
  return `Thu ${h % 12 || 12}:${m}:${sec} ${h < 12 ? "AM" : "PM"}`;
};

export function createDemo({ root, announce, sound, reducedMotion }) {
  const q = (selector) => root.querySelector(selector);
  const tray = q("#tray");
  const badge = q("#tray-badge");
  const slot = q("#tray-slot");
  const caret = q("#caret");
  const panel = q("#tray-panel");
  const desktop = q("#desktop");
  const logList = q("#log");
  const bannerStack = q("#banners");
  const playButton = q("#demo-play");
  const stepButton = q("#demo-step");
  const restartButton = q("#demo-restart");
  const progress = q("#demo-progress");
  const menuClock = q("#mb-clock");

  let state = initialState();
  let step = 0;
  let seconds = START_SECONDS;
  let elapsed = 0;
  let playing = false;
  let timer = 0;
  let view = "queue";
  let card = "ready";
  let follow = true;
  let selected = "";
  let banners = [];
  let resumeOnShow = false;
  let active = true;

  // Panel skeleton.
  const head = el("div", "tp-head");
  const brand = el("span");
  brand.append(scopeIcon(), " PR Sniper");
  head.append(brand, el("span", "", "Monitoring [ON]"));
  const title = el("h3", "tp-title");
  const cards = el("div", "tp-cards");
  const cardButtons = {};
  for (const [key, label] of [
    ["ready", "Ready for you"],
    ["attention", "Needs attention"],
  ]) {
    const button = el("button", "tp-card frame");
    button.type = "button";
    button.dataset.card = key;
    const name = el("span", "", label);
    const count = el("span", "tp-count", "0");
    button.append(name, count);
    frame(button);
    button.addEventListener("click", () => {
      follow = false;
      card = key;
      render();
    });
    cardButtons[key] = { button, count };
    cards.append(button);
  }
  const body = el("div", "tp-body");
  const list = el("ul", "tp-list");
  body.append(list);
  const sep = el("div", "tp-sep");
  sep.setAttribute("aria-hidden", "true");
  sep.append(
    el("span", "", "├"),
    el("span", "tp-sep-line", H),
    el("span", "", "┤"),
  );
  const nav = el("div", "tp-nav");
  nav.setAttribute("role", "tablist");
  nav.setAttribute("aria-label", "Panel");
  const navTabs = {};
  for (const name of VIEWS) {
    const tab = el("button");
    tab.type = "button";
    tab.setAttribute("role", "tab");
    tab.id = `tp-tab-${name}`;
    tab.dataset.view = name;
    tab.addEventListener("click", () => {
      follow = false;
      setView(name);
    });
    navTabs[name] = tab;
    nav.append(tab);
  }
  nav.append(el("span", "tp-version", "v0.4.0"));
  nav.addEventListener("keydown", (event) => {
    const index = VIEWS.indexOf(event.target.dataset.view);
    if (index < 0) return;
    let next = -1;
    if (event.key === "ArrowRight") next = (index + 1) % VIEWS.length;
    else if (event.key === "ArrowLeft")
      next = (index + VIEWS.length - 1) % VIEWS.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = VIEWS.length - 1;
    if (next < 0) return;
    event.preventDefault();
    follow = false;
    setView(VIEWS[next]);
    navTabs[VIEWS[next]].focus();
    sound.click();
  });
  body.setAttribute("role", "tabpanel");
  body.id = "tp-body";
  panel.append(head, title, cards, body, sep, nav);
  frame(panel);
  frame(q(".log"));

  function setView(name) {
    view = name;
    render();
  }

  function rowLines(lines) {
    const button = el("button", "row");
    button.type = "button";
    for (const [left, right, leftClass, rightClass] of lines) {
      const line = el("span", "row-line");
      line.append(el("span", leftClass || "", left));
      if (right) line.append(el("span", `right ${rightClass || "dim"}`, right));
      button.append(line);
    }
    return button;
  }

  function rowsFor() {
    if (view === "queue" && card === "ready") {
      return state.ready.map((item) => [
        `ready:${item.pr}`,
        [
          ["[Ready for you]", AGENTS[item.agent], "chip"],
          [PRS[item.pr].title],
          [ref(item.pr), `@${PRS[item.pr].author}`, "dim"],
        ],
      ]);
    }
    if (view === "queue") {
      return state.attention.map((item) => [
        `attention:${item.pr}`,
        [
          [`[${item.cause}]`, "[RETRY]", "chip", "chip"],
          [PRS[item.pr].title],
          [ref(item.pr), AGENTS[item.agent], "dim"],
        ],
      ]);
    }
    if (view === "running") {
      return state.running.map((item) => [
        `running:${item.pr}`,
        [
          [
            `${item.state === "Running" ? "●" : item.state === "Queued" ? "·" : "×"} ${AGENTS[item.agent]}`,
            item.state,
            "",
            item.state === "Running" ? "state-running" : "dim",
          ],
          [PRS[item.pr].title],
          [ref(item.pr), "Pass 1", "dim"],
        ],
      ]);
    }
    return state.reviewed.map((item) => [
      `reviewed:${item.pr}`,
      [
        [`[x] ${AGENTS[item.agent]}`, item.label, "", "dim"],
        [PRS[item.pr].title],
        [ref(item.pr), "Normal pass", "dim"],
      ],
    ]);
  }

  function render() {
    const focusedKey =
      document.activeElement && list.contains(document.activeElement)
        ? document.activeElement.dataset.key
        : "";
    title.textContent = VIEW_TITLES[view];
    cards.hidden = view !== "queue";
    cardButtons.ready.count.textContent = String(state.ready.length);
    cardButtons.attention.count.textContent = String(state.attention.length);
    for (const key of ["ready", "attention"]) {
      cardButtons[key].button.setAttribute(
        "aria-pressed",
        String(card === key),
      );
    }
    for (const name of VIEWS) {
      const tab = navTabs[name];
      const on = name === view;
      tab.setAttribute("aria-selected", String(on));
      tab.tabIndex = on ? 0 : -1;
      tab.textContent =
        name === "running" && state.running.length
          ? `Running ${state.running.length}`
          : VIEW_LABELS[name];
    }
    body.setAttribute("aria-labelledby", `tp-tab-${view}`);
    list.replaceChildren();
    for (const [key, lines] of rowsFor()) {
      const item = el("li");
      const row = rowLines(lines);
      row.dataset.key = key;
      if (key === selected) row.setAttribute("aria-current", "true");
      row.addEventListener("click", () => {
        selected = key;
        render();
        list.querySelector(`[data-key="${key}"]`)?.focus();
      });
      item.append(row);
      list.append(item);
    }
    if (focusedKey) list.querySelector(`[data-key="${focusedKey}"]`)?.focus();
    const waiting = state.ready.length + state.attention.length;
    badge.hidden = waiting === 0;
    badge.textContent = String(waiting);
    tray.setAttribute(
      "aria-label",
      waiting ? `PR Sniper, ${waiting} in queue` : "PR Sniper",
    );
    menuClock.textContent = clock(seconds, true);
    progress.textContent = `STEP ${String(step).padStart(2, "0")}/${SCRIPT.length}`;
    playButton.textContent = playing ? "[■ PAUSE]" : "[▶ PLAY]";
  }

  function log(text) {
    const item = el("li");
    const time = el("time", "", clock(seconds, false));
    item.append(time, el("span", "", text));
    logList.append(item);
    while (logList.children.length > 60) logList.firstElementChild.remove();
    logList.scrollTop = logList.scrollHeight;
  }

  function layoutBanners() {
    banners.forEach((banner, index) => {
      banner.el.classList.toggle("collapsed", index > 0);
    });
  }

  function dismiss(banner) {
    const hadFocus = banner.el.contains(document.activeElement);
    banner.el.remove();
    banners = banners.filter((item) => item !== banner);
    layoutBanners();
    if (hadFocus) (banners[0]?.el || tray).focus();
  }

  function notify(notice) {
    const text = NOTICES[notice.kind];
    const button = el("button", "banner frame");
    button.type = "button";
    button.dataset.kind = notice.kind;
    button.dataset.target = notice.key;
    const top = el("span", "banner-head");
    const name = el("span");
    name.append(scopeIcon(), " PR Sniper");
    top.append(name, el("span", "dim", "now"));
    button.append(
      top,
      el("span", "banner-title", text.title),
      el("span", "banner-body", text.body),
    );
    frame(button);
    const banner = { el: button, left: BANNER_MS };
    button.addEventListener("click", () => {
      dismiss(banner);
      open(notice);
    });
    bannerStack.prepend(button);
    banners.unshift(banner);
    while (banners.length > MAX_BANNERS) dismiss(banners[banners.length - 1]);
    layoutBanners();
    announce(`PR Sniper: ${text.title}. ${text.body}`);
    sound.chime();
  }

  function open(target) {
    setOpen(true);
    view = target.view;
    if (target.card) card = target.card;
    selected = target.key;
    render();
    const row = list.querySelector(`[data-key="${target.key}"]`);
    if (row) {
      row.scrollIntoView({ block: "nearest" });
      row.focus();
    }
  }

  function setOpen(open) {
    slot.hidden = !open;
    tray.setAttribute("aria-expanded", String(open));
    place();
  }

  function place() {
    if (slot.hidden) return;
    const desk = desktop.getBoundingClientRect();
    const icon = tray.getBoundingClientRect();
    const width = slot.getBoundingClientRect().width;
    const center = icon.left + icon.width / 2;
    const stacked = getComputedStyle(desktop).flexDirection === "column";
    const right = stacked
      ? 0
      : Math.max(0, Math.round(desk.right - (center + width / 2)));
    slot.style.setProperty("--slot-right", `${right}px`);
    const left = slot.getBoundingClientRect().left;
    const half = 8;
    const x = Math.min(
      Math.max(center - left - half, half * 2),
      width - half * 4,
    );
    slot.style.setProperty("--caret-x", `${x}px`);
  }

  function apply(index, quiet) {
    const entry = SCRIPT[index];
    seconds = START_SECONDS + entry.at;
    if (entry.run) entry.run(state);
    if (follow && entry.view) {
      view = entry.view;
      if (entry.card) card = entry.card;
    }
    log(entry.log);
    if (entry.notice && !quiet) notify(entry.notice);
  }

  function advance() {
    if (step >= SCRIPT.length) {
      reset();
      return;
    }
    const entry = SCRIPT[step];
    apply(step, false);
    step += 1;
    elapsed = 0;
    if (entry.restart) {
      render();
      step = SCRIPT.length;
      return;
    }
    render();
  }

  function reset() {
    state = initialState();
    step = 0;
    seconds = START_SECONDS;
    elapsed = 0;
    selected = "";
    view = "queue";
    card = "ready";
    follow = true;
    logList.replaceChildren();
    for (const banner of [...banners]) dismiss(banner);
    log("PR Sniper 0.4.0 · Monitoring on · 3 repositories");
    render();
  }

  function tick() {
    elapsed += TICK;
    if (elapsed % 1000 === 0) {
      seconds += 1;
      menuClock.textContent = clock(seconds, true);
    }
    for (const banner of [...banners]) {
      banner.left -= TICK;
      if (banner.left <= 0) dismiss(banner);
    }
    if (elapsed >= STEP_MS) advance();
  }

  function play() {
    if (playing) return;
    playing = true;
    timer = window.setInterval(tick, TICK);
    render();
  }

  function pause() {
    if (!playing) return;
    playing = false;
    window.clearInterval(timer);
    render();
  }

  tray.addEventListener("click", () => {
    setOpen(slot.hidden);
    sound.click();
  });
  playButton.addEventListener("click", () => (playing ? pause() : play()));
  stepButton.addEventListener("click", () => {
    pause();
    advance();
  });
  restartButton.addEventListener("click", () => {
    reset();
    if (reducedMotion) pause();
  });
  if ("ResizeObserver" in window) new ResizeObserver(place).observe(desktop);
  window.addEventListener("resize", place);
  document.fonts?.ready.then(place);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      resumeOnShow = resumeOnShow || playing;
      pause();
    } else if (resumeOnShow && active) {
      resumeOnShow = false;
      play();
    }
  });

  reset();
  if (reducedMotion) {
    for (let index = 0; index < MID_STEP; index += 1) apply(index, true);
    step = MID_STEP;
    render();
  }
  place();

  return {
    start() {
      if (reducedMotion) return;
      if (active) play();
      else resumeOnShow = true;
    },
    toggle() {
      if (playing) pause();
      else play();
    },
    setActive(visible) {
      active = visible;
      if (!visible) {
        resumeOnShow = resumeOnShow || playing;
        pause();
      } else {
        place();
        if (resumeOnShow) {
          resumeOnShow = false;
          play();
        }
      }
    },
  };
}
