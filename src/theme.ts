export type Theme = "light" | "dark" | "os";

export const themeOptions: readonly (readonly [Theme, string])[] = [
  ["light", "Light"],
  ["dark", "Dark"],
  ["os", "OS"],
];

// Paint hint only; the saved Settings value is authoritative once read.
const hintKey = "pr-sniper-theme";
const system = window.matchMedia("(prefers-color-scheme: dark)");
let preference: Theme = "light";

export const isTheme = (value: unknown): value is Theme =>
  value === "light" || value === "dark" || value === "os";

function render() {
  const dark = preference === "dark" || (preference === "os" && system.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

export function currentTheme(): Theme {
  return preference;
}

export function applyTheme(next: unknown) {
  preference = isTheme(next) ? next : "light";
  render();
  try {
    localStorage.setItem(hintKey, preference);
  } catch {
    // Storage may be unavailable; the saved setting still applies on read.
  }
}

system.addEventListener("change", () => {
  if (preference === "os") render();
});

let hint: string | null = null;
try {
  hint = localStorage.getItem(hintKey);
} catch {
  hint = null;
}
preference = isTheme(hint) ? hint : "light";
render();
