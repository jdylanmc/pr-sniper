import { createDemo } from "./demo.js";

// Room kept below the desktop so it fits under the header.
const BOTTOM_GAP = 24;
const stage = document.getElementById("demo-stage");

const demo = createDemo({
  fitHeight: (padY) => {
    const top = stage.getBoundingClientRect().top + window.scrollY;
    const height = window.innerHeight - top - padY - BOTTOM_GAP;
    return height < 160 ? Infinity : height;
  },
});
demo.load();
demo.fit();

// This page is the plain look only; turning thematic on opens the terminal's
// DEMO tab.
function thematicOn() {
  try {
    localStorage.setItem("termlink-thematic", "on");
    sessionStorage.setItem("termlink-booted", "1");
  } catch {
    // Storage can be refused; the landing page opens instead.
  }
  location.href = "index.html#demo";
}
document.getElementById("thematic-btn").addEventListener("click", thematicOn);

document.addEventListener("keydown", (event) => {
  if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey)
    return;
  if (demo.isFull()) {
    if (event.key === "Escape") {
      demo.exit();
      event.preventDefault();
    }
    return;
  }
  if (event.key === "h" || event.key === "H") {
    event.preventDefault();
    thematicOn();
  }
});
