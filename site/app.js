import { frame } from "./frame.js";

const THEMES = [
  { id: "p1", name: "P1 GREEN", bg: "#021a0a" },
  { id: "p3", name: "P3 AMBER", bg: "#1a0f00" },
  { id: "p4", name: "P4 WHITE", bg: "#0a0c10" },
  { id: "p7", name: "P7 BLUE", bg: "#031422" },
];
const THEME_KEY = "termlink-theme";
const BOOT_KEY = "termlink-booted";
const REPO_BLOB = "https://github.com/jdylanmc/pr-sniper/blob/main/";
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;

const storage = (kind) => {
  try {
    return window[kind];
  } catch {
    return null;
  }
};
const read = (kind, key) => {
  try {
    return storage(kind)?.getItem(key) ?? null;
  } catch {
    return null;
  }
};
const write = (kind, key, value) => {
  try {
    storage(kind)?.setItem(key, value);
  } catch {
    // Private browsing can refuse storage; the page still works.
  }
};

const announcer = document.getElementById("announcer");
function announce(text) {
  announcer.textContent = "";
  window.setTimeout(() => (announcer.textContent = text), 50);
}

// Sound: synthesized with Web Audio, off until the user turns it on.
const sound = {
  on: false,
  ctx: null,
  context() {
    if (!this.ctx) {
      const Context = window.AudioContext || window.webkitAudioContext;
      if (!Context) return null;
      this.ctx = new Context();
    }
    if (this.ctx.state === "suspended") this.ctx.resume();
    return this.ctx;
  },
  tone(frequency, start, length, peak, type = "sine") {
    const ctx = this.context();
    if (!ctx) return;
    const t = ctx.currentTime + start;
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = type;
    osc.frequency.setValueAtTime(frequency, t);
    gain.gain.setValueAtTime(0.0001, t);
    gain.gain.exponentialRampToValueAtTime(
      peak,
      t + Math.min(0.02, length / 4),
    );
    gain.gain.exponentialRampToValueAtTime(0.0001, t + length);
    osc.connect(gain).connect(ctx.destination);
    osc.start(t);
    osc.stop(t + length + 0.02);
  },
  click() {
    if (!this.on) return;
    const ctx = this.context();
    if (!ctx) return;
    const length = Math.floor(ctx.sampleRate * 0.025);
    const buffer = ctx.createBuffer(1, length, ctx.sampleRate);
    const data = buffer.getChannelData(0);
    for (let i = 0; i < length; i += 1) {
      data[i] = (Math.random() * 2 - 1) * (1 - i / length) ** 3;
    }
    const source = ctx.createBufferSource();
    const filter = ctx.createBiquadFilter();
    const gain = ctx.createGain();
    source.buffer = buffer;
    filter.type = "bandpass";
    filter.frequency.value = 2400;
    gain.gain.value = 0.25;
    source.connect(filter).connect(gain).connect(ctx.destination);
    source.start();
  },
  hum() {
    if (!this.on) return;
    this.tone(60, 0, 1.2, 0.05, "sawtooth");
    this.tone(120, 0, 1.0, 0.03);
    this.tone(15600, 0, 0.8, 0.004);
  },
  chime() {
    if (!this.on) return;
    this.tone(880, 0, 0.35, 0.05);
    this.tone(1318.5, 0.12, 0.45, 0.04);
  },
};

const soundButton = document.getElementById("sound-btn");
function toggleSound() {
  sound.on = !sound.on;
  soundButton.textContent = sound.on ? "[SOUND ON]" : "[SOUND OFF]";
  soundButton.setAttribute("aria-pressed", String(sound.on));
  if (sound.on) sound.hum();
}
soundButton.addEventListener("click", toggleSound);

// Themes: always start on P1 unless the visitor picked another.
const themeButton = document.getElementById("theme-btn");
const themeMeta = document.querySelector('meta[name="theme-color"]');
function setTheme(id, save) {
  const theme = THEMES.find((item) => item.id === id) || THEMES[0];
  document.body.dataset.theme = theme.id;
  themeButton.textContent = `[THEME ${theme.name}]`;
  themeMeta.setAttribute("content", theme.bg);
  if (save) {
    write("localStorage", THEME_KEY, theme.id);
    announce(`Theme ${theme.name}`);
  }
}
function cycleTheme() {
  const index = THEMES.findIndex(
    (item) => item.id === document.body.dataset.theme,
  );
  setTheme(THEMES[(index + 1) % THEMES.length].id, true);
}
themeButton.addEventListener("click", cycleTheme);
setTheme(read("localStorage", THEME_KEY) || "p1", false);

// Frames
for (const node of document.querySelectorAll(".info, .main, .demo-screen"))
  frame(node);

// Demo: a 1440x810 desktop scaled uniformly to fit, always 16:9.
const DEMO_W = 1440;
const DEMO_H = 810;
const demoStage = document.getElementById("demo-stage");
const demoViewport = document.getElementById("demo-viewport");
const demoScreen = demoViewport.parentElement;
const stacked = matchMedia("(max-width: 900px)");

function fitDemo() {
  if (!demoStage.getClientRects().length) return;
  const screen = getComputedStyle(demoScreen);
  const padX = parseFloat(screen.paddingLeft) + parseFloat(screen.paddingRight);
  const padY = parseFloat(screen.paddingTop) + parseFloat(screen.paddingBottom);
  const width = demoStage.clientWidth - padX;
  const height =
    stacked.matches && !isDemoFull() ? Infinity : demoStage.clientHeight - padY;
  const scale = Math.max(0.1, Math.min(width / DEMO_W, height / DEMO_H));
  demoViewport.style.width = `${Math.floor(DEMO_W * scale)}px`;
  demoViewport.firstElementChild.style.transform = `scale(${scale})`;
}

new ResizeObserver(fitDemo).observe(demoStage);
stacked.addEventListener?.("change", fitDemo);

// Full screen: the browser's Fullscreen API where it exists (Esc exits
// natively), else a fixed overlay that Esc closes from the page or the demo.
const demoFull = document.getElementById("demo-full");
const nativeFull = Boolean(
  demoStage.requestFullscreen || demoStage.webkitRequestFullscreen,
);

function fullElement() {
  return document.fullscreenElement || document.webkitFullscreenElement;
}

function isDemoFull() {
  return (
    fullElement() === demoStage || demoStage.classList.contains("demo-overlay")
  );
}

function syncFull() {
  const on = isDemoFull();
  demoFull.setAttribute("aria-pressed", String(on));
  demoFull.textContent = on ? "[EXIT FULL SCREEN · ESC]" : "[FULL SCREEN]";
  fitDemo();
}

function enterFull() {
  if (nativeFull) {
    const request =
      demoStage.requestFullscreen || demoStage.webkitRequestFullscreen;
    Promise.resolve(request.call(demoStage)).catch(() => {
      demoStage.classList.add("demo-overlay");
      syncFull();
    });
  } else {
    demoStage.classList.add("demo-overlay");
    syncFull();
  }
}

function exitFull() {
  if (demoStage.classList.contains("demo-overlay")) {
    demoStage.classList.remove("demo-overlay");
    syncFull();
    demoFull.focus();
  } else if (fullElement()) {
    (document.exitFullscreen || document.webkitExitFullscreen).call(document);
  }
}

demoFull.addEventListener("click", () => {
  if (isDemoFull()) exitFull();
  else enterFull();
  sound.click();
});
document.addEventListener("fullscreenchange", syncFull);
document.addEventListener("webkitfullscreenchange", syncFull);

// Esc inside the (same-origin) demo also closes the overlay fallback.
document.getElementById("demo-frame").addEventListener("load", (event) => {
  try {
    event.target.contentDocument.addEventListener("keydown", (inner) => {
      if (
        inner.key === "Escape" &&
        demoStage.classList.contains("demo-overlay")
      )
        exitFull();
    });
  } catch {
    // Cross-origin in some previews; the page-level Esc still works.
  }
});

// Tabs
const tabs = [...document.querySelectorAll('.tabs [role="tab"]')];
const scroller = document.getElementById("scroller");
const names = tabs.map((tab) => tab.id.replace("tab-", ""));
let current = 0;
let redrawOnSwitch = false;

function selectTab(index, focus) {
  const previous = current;
  current = (index + tabs.length) % tabs.length;
  tabs.forEach((tab, i) => {
    const on = i === current;
    tab.setAttribute("aria-selected", String(on));
    tab.tabIndex = on ? 0 : -1;
    document.getElementById(tab.getAttribute("aria-controls")).hidden = !on;
  });
  if (focus) tabs[current].focus();
  // Redraw the newly shown section like a terminal printing it.
  const shown = document.getElementById(
    tabs[current].getAttribute("aria-controls"),
  );
  if (redrawOnSwitch && previous !== current && !reducedMotion) {
    shown.classList.remove("redraw");
    void shown.offsetWidth;
    shown.classList.add("redraw");
  }
  scroller.scrollTop = 0;
  if (names[current] === "changelog") loadChangelog();
  if (names[current] === "demo") {
    const demoFrame = document.getElementById("demo-frame");
    if (!demoFrame.src) demoFrame.src = demoFrame.dataset.src;
    fitDemo();
  }
  const hash = current === 0 ? "" : `#${names[current]}`;
  if (location.hash !== hash) {
    history.replaceState(null, "", hash || location.pathname + location.search);
  }
}

for (const panel of document.querySelectorAll('[role="tabpanel"]')) {
  panel.addEventListener("animationend", (event) => {
    if (event.animationName === "redraw") panel.classList.remove("redraw");
  });
}

tabs.forEach((tab, index) => {
  tab.addEventListener("click", () => {
    selectTab(index, false);
    sound.click();
  });
});

function scrollMain(direction) {
  const step =
    direction * parseFloat(getComputedStyle(document.body).lineHeight) * 3;
  const inner = getComputedStyle(scroller).overflowY;
  if (
    (inner === "auto" || inner === "scroll") &&
    scroller.scrollHeight > scroller.clientHeight
  ) {
    scroller.scrollBy(0, step);
  } else {
    window.scrollBy(0, step);
  }
}

function isTyping(target) {
  return Boolean(
    target.closest?.(
      'input, textarea, select, [contenteditable=""], [contenteditable="true"]',
    ),
  );
}

document.addEventListener("keydown", (event) => {
  if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey)
    return;
  if (booting) {
    endBoot();
    event.preventDefault();
    return;
  }
  if (isDemoFull()) {
    if (event.key === "Escape") {
      exitFull();
      event.preventDefault();
    }
    return;
  }
  const target = event.target;
  if (isTyping(target)) return;
  const onTab = tabs.includes(target);
  const key = event.key;
  let handled = true;
  if (key === "ArrowRight") selectTab(current + 1, onTab);
  else if (key === "ArrowLeft") selectTab(current - 1, onTab);
  else if (onTab && key === "Home") selectTab(0, true);
  else if (onTab && key === "End") selectTab(tabs.length - 1, true);
  else if (/^[1-5]$/.test(key)) selectTab(Number(key) - 1, onTab);
  else if (key === "ArrowDown") scrollMain(1);
  else if (key === "ArrowUp") scrollMain(-1);
  else if (key === "t" || key === "T") cycleTheme();
  else if (key === "m" || key === "M") toggleSound();
  else handled = false;
  if (handled) {
    event.preventDefault();
    sound.click();
  }
});

// Copy buttons
async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.append(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand("copy");
    } catch {
      ok = false;
    }
    area.remove();
    return ok;
  }
}

for (const button of document.querySelectorAll(".copy")) {
  button.addEventListener("click", async () => {
    const text = button
      .closest(".cmd")
      .querySelector("code")
      .textContent.trim();
    const ok = await copyText(text);
    button.textContent = ok ? "[Copied]" : "[Copy failed]";
    announce(ok ? "Copied" : "Copy failed");
    window.clearTimeout(button.resetTimer);
    button.resetTimer = window.setTimeout(
      () => (button.textContent = "[COPY]"),
      2000,
    );
  });
}

// Changelog: a small Markdown subset rendered with text nodes only.
let changelogLoaded = false;

function safeHref(href) {
  if (/^https?:\/\//i.test(href)) return href;
  if (/^[a-z][a-z\d+.-]*:/i.test(href) || href.startsWith("//")) return "";
  return REPO_BLOB + href.replace(/^\.?\//, "");
}

function inline(text, parent, links = true) {
  const pattern = /`([^`]+)`|\*\*([^*]+)\*\*|\[([^\]]+)\]\(([^)\s]+)\)/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > last) parent.append(text.slice(last, match.index));
    if (match[1] !== undefined) {
      const code = document.createElement("code");
      code.textContent = match[1];
      parent.append(code);
    } else if (match[2] !== undefined) {
      const strong = document.createElement("strong");
      inline(match[2], strong, links);
      parent.append(strong);
    } else {
      const href = links ? safeHref(match[4]) : "";
      if (href) {
        const anchor = document.createElement("a");
        anchor.href = href;
        inline(match[3], anchor, false);
        parent.append(anchor);
      } else {
        inline(match[3], parent, false);
      }
    }
    last = match.index + match[0].length;
  }
  if (last < text.length) parent.append(text.slice(last));
}

export function renderMarkdown(source, root) {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  let block = null;
  let list = null;
  let nested = null;
  let fence = null;
  const finish = () => {
    if (block) inline(block.text, block.node);
    block = null;
  };
  const closeLists = () => {
    finish();
    list = null;
    nested = null;
  };
  for (const line of lines) {
    if (fence) {
      if (/^```/.test(line)) fence = null;
      else fence.textContent += `${line}\n`;
      continue;
    }
    if (/^```/.test(line)) {
      closeLists();
      fence = document.createElement("pre");
      root.append(fence);
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      closeLists();
      const level = heading[1].length;
      if (level === 1) continue;
      const node = document.createElement(`h${Math.min(6, level + 1)}`);
      inline(heading[2].trim(), node);
      root.append(node);
      continue;
    }
    const bullet = /^(\s*)[-*]\s+(.*)$/.exec(line);
    if (bullet) {
      finish();
      const item = document.createElement("li");
      if (bullet[1].length >= 2 && list) {
        if (!nested) {
          nested = document.createElement("ul");
          list.lastElementChild?.append(nested);
        }
        nested.append(item);
      } else {
        if (!list) {
          list = document.createElement("ul");
          root.append(list);
        }
        nested = null;
        list.append(item);
      }
      block = { node: item, text: bullet[2] };
      continue;
    }
    if (!line.trim()) {
      closeLists();
      continue;
    }
    if (block) {
      block.text += ` ${line.trim()}`;
      continue;
    }
    list = null;
    const paragraph = document.createElement("p");
    root.append(paragraph);
    block = { node: paragraph, text: line.trim() };
  }
  finish();
}

async function fetchChangelog() {
  for (const url of ["CHANGELOG.md", "../CHANGELOG.md"]) {
    try {
      const response = await fetch(url, { cache: "no-cache" });
      if (!response.ok) continue;
      const text = await response.text();
      if (text.startsWith("# Changelog")) return text;
    } catch {
      // Try the next location.
    }
  }
  return null;
}

async function loadChangelog() {
  if (changelogLoaded) return;
  changelogLoaded = true;
  const root = document.getElementById("changelog");
  const text = await fetchChangelog();
  root.replaceChildren();
  if (!text) {
    const p = document.createElement("p");
    p.append("Changelog unavailable. Read it on ");
    const a = document.createElement("a");
    a.href = `${REPO_BLOB}CHANGELOG.md`;
    a.textContent = "GitHub";
    p.append(a, ".");
    root.append(p);
    changelogLoaded = false;
    return;
  }
  renderMarkdown(text, root);
}

// Header uptime
const uptime = document.getElementById("uptime");
const started = Date.now();
window.setInterval(() => {
  const total = Math.floor((Date.now() - started) / 1000);
  const parts = [
    Math.floor(total / 3600),
    Math.floor(total / 60) % 60,
    total % 60,
  ];
  uptime.textContent = parts.map((n) => String(n).padStart(2, "0")).join(":");
}, 1000);

// Boot: once per session, skipped under reduced motion.
const BOOT_LINES = [
  "TERMLINK BIOS 0.4.0 · PR SNIPER PROJECT",
  "",
  "MEMORY TEST ............ 65536K OK",
  "LOADING SCOPE.DRV ...... OK",
  "MOUNTING /QUEUE ........ OK",
  "AGENTS ................. STANDING BY",
  "PERMISSIONS ............ ALL OFF",
  "FINAL CALL ............. HUMAN",
  "",
  "> STARTING TERMLINK _",
];
const bootNode = document.getElementById("boot");
const bootText = document.getElementById("boot-text");
let booting = false;
let bootTimers = [];

function endBoot() {
  if (!booting) return;
  booting = false;
  bootTimers.forEach((id) => window.clearTimeout(id));
  bootTimers = [];
  bootNode.hidden = true;
  document.body.classList.add("booting");
  window.setTimeout(() => document.body.classList.remove("booting"), 800);
}

function boot() {
  if (reducedMotion || read("sessionStorage", BOOT_KEY)) return;
  write("sessionStorage", BOOT_KEY, "1");
  booting = true;
  bootNode.hidden = false;
  frame(bootNode);
  sound.hum();
  BOOT_LINES.forEach((line, index) => {
    bootTimers.push(
      window.setTimeout(
        () => {
          bootText.textContent += `${line}\n`;
          sound.click();
        },
        120 + index * 160,
      ),
    );
  });
  bootTimers.push(window.setTimeout(endBoot, 2100));
  bootNode.addEventListener("pointerdown", endBoot);
}

const initial = names.indexOf(location.hash.slice(1));
selectTab(initial > 0 ? initial : 0, false);
redrawOnSwitch = true;
boot();
