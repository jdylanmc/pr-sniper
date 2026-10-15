// A background refresh must not replace the node under a pressed pointer:
// WebKit (and Chromium, for removed elements) then drops the click.

/** Writes text only when it changed, keeping the existing text node. */
export function setText(element: Element, text: string) {
  if (element.textContent !== text) element.textContent = text;
}

/** Longest a background redraw waits for a press that never reports a
 * release (context menus, drags, focus changes). */
export const MAX_PRESS_DEFERRAL_MS = 1500;

let pressed = false;
let deadline: ReturnType<typeof setTimeout> | undefined;
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
  clearTimeout(deadline);
  deadline = undefined;
  if (!pressed) return;
  pressed = false;
  channel.port2.postMessage(null);
};
const mac = /Mac/.test(navigator.platform);
addEventListener(
  "pointerdown",
  (event) => {
    // Only a primary click can be dropped; secondary and macOS Control
    // clicks open a context menu that swallows the release.
    if (event.button !== 0 || (mac && event.ctrlKey)) return;
    pressed = true;
    clearTimeout(deadline);
    deadline = setTimeout(release, MAX_PRESS_DEFERRAL_MS);
  },
  true,
);
for (const type of [
  "pointerup",
  "pointercancel",
  "contextmenu",
  "dragstart",
  "dragend",
  "keydown",
])
  addEventListener(type, release, true);
// A release outside the window is never delivered; leaving it ends the press.
addEventListener("blur", release);

/** Runs a background redraw now, or once the current primary press ends (at
 * most `MAX_PRESS_DEFERRAL_MS` later). The latest call for a task wins. */
export function whenReleased(task: () => void) {
  if (!pressed) {
    task();
    return;
  }
  waiting.add(task);
}

/** Runs a user-initiated redraw immediately, superseding a deferred one. */
export function drawNow(task: () => void) {
  waiting.delete(task);
  task();
}
