import { listen } from "@tauri-apps/api/event";

// Native Store commits from any origin: this window, another window, local
// control or background work. See `src-tauri/src/changes.rs`.
export const SETTINGS_CHANGED = "pr-sniper:settings-changed";
export const STATE_CHANGED = "pr-sniper:state-changed";

/** Runs `refresh` once per burst of `events` while `root` stays connected. */
export function onCommitted(
  root: Element,
  events: readonly string[],
  refresh: () => void,
  delay = 50,
) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const stops: (() => void)[] = [];
  let stopped = false;
  const stop = () => {
    stopped = true;
    clearTimeout(timer);
    stops.splice(0).forEach((unlisten) => unlisten());
  };
  const schedule = () => {
    if (!root.isConnected) return stop();
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (root.isConnected) refresh();
      else stop();
    }, delay);
  };
  for (const event of events)
    void listen(event, schedule).then(
      (unlisten) => (stopped ? unlisten() : stops.push(unlisten)),
      () => {},
    );
  return stop;
}
