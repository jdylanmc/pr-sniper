// The hero's mini panel: PR Sniper's Queue acting out the tagline. A new
// pull request starts reviewing, the scope locks onto it, it turns Ready for
// you and the count goes up; the oldest one leaves and the loop repeats.

const SVG = "http://www.w3.org/2000/svg";
const ICONS = {
  check: "m5 12 4 4L19 6",
  chevron: "m9 5 7 7-7 7",
};
const PULL =
  '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="19" r="2"/><path d="M6 7v10M18 17v-6a6 6 0 0 0-6-6m0 0 3-3m-3 3 3 3"/>';

const INCOMING = [
  { title: "Fix token refresh race", repo: "lumen/ledger", number: 142 },
  { title: "Escape markdown in titles", repo: "quill/inkwell", number: 58 },
  { title: "Retry webhook on 502", repo: "harbor/dock", number: 311 },
  { title: "Drop stale cache keys", repo: "orbit/relay", number: 77 },
];

function icon(markup, className = "mini-icon") {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", className);
  svg.innerHTML = markup;
  return svg;
}

function row(pr, reviewing) {
  const li = document.createElement("li");
  li.className = "mini-row";
  const inner = document.createElement("div");
  inner.className = "mini-row-inner";
  const copy = document.createElement("span");
  copy.className = "mini-row-copy";
  const pill = document.createElement("span");
  pill.className = "mini-pill";
  const title = document.createElement("span");
  title.className = "mini-row-title";
  title.textContent = pr.title;
  const ref = document.createElement("span");
  ref.className = "mini-ref";
  ref.append(icon(PULL), `${pr.repo} #${pr.number}`);
  copy.append(pill, title, ref);
  inner.append(copy, icon(`<path d="${ICONS.chevron}"/>`, "mini-chevron"));
  li.append(
    inner,
    Object.assign(document.createElement("span"), {
      className: "mini-shimmer",
    }),
  );
  setState(li, reviewing);
  return li;
}

function setState(li, reviewing) {
  const pill = li.querySelector(".mini-pill");
  pill.replaceChildren();
  li.classList.toggle("is-reviewing", reviewing);
  if (reviewing) {
    const spin = document.createElement("span");
    spin.className = "mini-spin";
    pill.append(spin, "Reviewing");
  } else {
    pill.append(icon(`<path d="${ICONS.check}"/>`), "Ready for you");
  }
}

export function startMini(root) {
  if (!root) return;
  const list = root.querySelector(".mini-list");
  const count = root.querySelector(".mini-count");
  const badge = root.querySelector(".mini-badge");
  const reticle = root.querySelector(".mini-reticle");
  const note = root.querySelector(".mini-note");
  const noteRef = root.querySelector(".mini-note-ref");
  let next = 0;
  let cycle = 0;
  let active = null;

  const setStep = (step) => (root.dataset.step = step);
  function setCount(node, value) {
    node.textContent = String(value);
    node.classList.remove("is-bumped");
    void node.offsetWidth;
    node.classList.add("is-bumped");
  }
  const readyRows = () => list.querySelectorAll(".mini-row:not(.is-reviewing)");

  // Offsets are layout pixels, so the panel's scale does not matter.
  const aim = (li) =>
    (reticle.style.top = `${li.offsetTop + li.offsetHeight / 2}px`);

  function enter() {
    active = row(INCOMING[next], true);
    active.classList.add("is-entering");
    list.prepend(active);
    void active.offsetWidth;
    active.classList.remove("is-entering");
    setCount(badge, 1);
  }

  function lock() {
    aim(active);
    reticle.classList.add("is-locked");
    active.classList.add("is-targeted");
  }

  function ready() {
    reticle.classList.remove("is-locked");
    active.classList.remove("is-targeted");
    setState(active, false);
    setCount(badge, 0);
  }

  function notify() {
    const pr = INCOMING[next];
    noteRef.textContent = `${pr.repo} #${pr.number}`;
    note.classList.add("is-shown");
  }

  function leave() {
    const rows = list.querySelectorAll(".mini-row");
    rows[rows.length - 1]?.classList.add("is-leaving");
  }

  function settle() {
    list.querySelector(".is-leaving")?.remove();
    setCount(count, readyRows().length);
  }

  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    // A still of the lock-on moment.
    active = row(INCOMING[next], true);
    list.prepend(active);
    badge.textContent = "1";
    lock();
    setStep("static");
    // Hidden while thematic is on; aim again once shown.
    new IntersectionObserver(() => aim(active)).observe(root);
    return;
  }

  // [ms into the cycle, step name, action]
  const STEPS = [
    [600, "enter", enter],
    [4200, "lock", lock],
    [5600, "ready", ready],
    [6100, "handoff", () => setCount(count, readyRows().length)],
    [6700, "notify", notify],
    [9000, "notified", () => note.classList.remove("is-shown")],
    [9700, "reset", leave],
    [10200, "settle", settle],
    [
      11000,
      "rest",
      () => {
        next = (next + 1) % INCOMING.length;
        cycle += 1;
        root.dataset.cycle = String(cycle);
      },
    ],
  ];

  let index = 0;
  let elapsed = 0;
  let startedAt = 0;
  let timer = 0;
  let visible = false;

  function schedule() {
    const [at, step, action] = STEPS[index];
    timer = window.setTimeout(
      () => {
        elapsed = at;
        startedAt = performance.now() - elapsed;
        action();
        setStep(step);
        index += 1;
        if (index === STEPS.length) {
          index = 0;
          elapsed = 0;
          startedAt = performance.now();
        }
        schedule();
      },
      Math.max(0, at - elapsed),
    );
  }

  function update() {
    const run = visible && !document.hidden;
    const running = root.classList.contains("is-running");
    if (run === running) return;
    root.classList.toggle("is-running", run);
    if (run) {
      startedAt = performance.now() - elapsed;
      schedule();
    } else {
      window.clearTimeout(timer);
      elapsed = Math.min(performance.now() - startedAt, STEPS[index][0]);
    }
  }

  root.dataset.cycle = "0";
  setStep("rest");
  new IntersectionObserver((entries) => {
    visible = entries[entries.length - 1].isIntersecting;
    update();
  }).observe(root);
  document.addEventListener("visibilitychange", update);
}
