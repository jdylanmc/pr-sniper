// A background refresh must not replace the node under a pressed pointer:
// WebKit (and Chromium, for removed elements) then drops the click.

/** Writes text only when it changed, keeping the existing text node. */
export function setText(element: Element, text: string) {
  if (element.textContent !== text) element.textContent = text;
}

let pressed = false;
const waiting = new Set<() => void>();
const flush = () => {
  if (pressed) return;
  const tasks = [...waiting];
  waiting.clear();
  for (const task of tasks) task();
};
// A macrotask after release runs once the release's click has dispatched;
// MessageChannel is independent of page timers.
const channel = new MessageChannel();
channel.port1.onmessage = flush;
const release = () => {
  pressed = false;
  channel.port2.postMessage(null);
};
addEventListener("pointerdown", () => (pressed = true), true);
addEventListener("pointerup", release, true);
addEventListener("pointercancel", release, true);
// A release outside the window is never delivered; leaving it ends the press.
addEventListener("blur", release);

/** Runs a redraw now, or right after the current press is released. The
 * latest call for the same task wins. */
export function whenReleased(task: () => void) {
  if (!pressed) {
    task();
    return;
  }
  waiting.add(task);
}
