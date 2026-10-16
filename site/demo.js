// The v2 prototype desktop: 1440x810 scaled uniformly to fit, always 16:9,
// with full screen. Shared by index.html (DEMO tab) and demo.html.
const DEMO_W = 1440;
const DEMO_H = 810;

// fitHeight(padY) returns the height available outside full screen.
export function createDemo({ fitHeight, onToggle = () => {} }) {
  const stage = document.getElementById("demo-stage");
  const viewport = document.getElementById("demo-viewport");
  const screen = viewport.parentElement;
  const button = document.getElementById("demo-full");
  const frame = document.getElementById("demo-frame");
  // The browser's Fullscreen API where it exists (Esc exits natively), else
  // a fixed overlay that Esc closes from the page or the demo.
  const nativeFull = Boolean(
    stage.requestFullscreen || stage.webkitRequestFullscreen,
  );

  const fullElement = () =>
    document.fullscreenElement || document.webkitFullscreenElement;
  const isFull = () =>
    fullElement() === stage || stage.classList.contains("demo-overlay");

  function fit() {
    if (!stage.getClientRects().length) return;
    const style = getComputedStyle(screen);
    const padX = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
    const padY = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
    const width = stage.clientWidth - padX;
    const height = isFull() ? stage.clientHeight - padY : fitHeight(padY);
    const scale = Math.max(0.1, Math.min(width / DEMO_W, height / DEMO_H));
    viewport.style.width = `${Math.floor(DEMO_W * scale)}px`;
    viewport.firstElementChild.style.transform = `scale(${scale})`;
  }

  function sync() {
    const on = isFull();
    button.setAttribute("aria-pressed", String(on));
    button.setAttribute("aria-label", on ? "Exit full screen" : "Full screen");
    const label = button.querySelector(".t-on");
    if (label) label.textContent = on ? "[EXIT]" : "[FULL]";
    fit();
  }

  function enter() {
    const overlay = () => {
      stage.classList.add("demo-overlay");
      sync();
    };
    if (!nativeFull) return overlay();
    const request = stage.requestFullscreen || stage.webkitRequestFullscreen;
    Promise.resolve(request.call(stage)).catch(overlay);
  }

  function exit() {
    if (stage.classList.contains("demo-overlay")) {
      stage.classList.remove("demo-overlay");
      sync();
      button.focus();
    } else if (fullElement()) {
      (document.exitFullscreen || document.webkitExitFullscreen).call(document);
    }
  }

  function load() {
    if (!frame.src) frame.src = frame.dataset.src;
  }

  button.addEventListener("click", () => {
    if (isFull()) exit();
    else enter();
    onToggle();
  });
  document.addEventListener("fullscreenchange", sync);
  document.addEventListener("webkitfullscreenchange", sync);
  // Esc inside the (same-origin) demo also closes the overlay fallback.
  frame.addEventListener("load", () => {
    try {
      frame.contentDocument.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && stage.classList.contains("demo-overlay"))
          exit();
      });
    } catch {
      // Cross-origin in some previews; the page-level Esc still works.
    }
  });
  new ResizeObserver(fit).observe(stage);
  window.addEventListener("resize", fit);

  return { fit, load, isFull, exit };
}
