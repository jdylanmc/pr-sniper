import { panelCases } from "./cases/panel.mjs";

export const cases = panelCases;
export const coverageLimits = [
  "Only panel navigation and unsaved-draft retention are registered.",
  "Native close, exact PR/job Back, focus, notifications, geometry, login, provider and other feature coverage remain unregistered.",
  "No macOS guest driver or Windows runner is implemented or proven.",
  "Unit, Rust integration, browser and release checks remain required separately.",
];

export function validateRegistry(registry = cases) {
  if (!Array.isArray(registry) || registry.length === 0)
    throw new Error("Registry must contain executable cases");
  const ids = new Set();
  const text = (value) => typeof value === "string" && value.trim().length > 0;
  const texts = (values) =>
    Array.isArray(values) && values.length > 0 && values.every(text);
  for (const entry of registry) {
    if (
      !entry ||
      !/^[a-z][a-z0-9-]+$/.test(entry.id) ||
      !/^[a-z][a-z0-9-]+$/.test(entry.feature) ||
      entry.feature === "full" ||
      ids.has(entry.id) ||
      typeof entry.run !== "function" ||
      !text(entry.expectation) ||
      !text(entry.source) ||
      !texts(entry.preconditions) ||
      !texts(entry.cleanup)
    )
      throw new Error("Invalid or duplicate executable case registration");
    ids.add(entry.id);
  }
}

export function selectCases(requested, registry = cases) {
  validateRegistry(registry);
  if (
    !Array.isArray(requested) ||
    requested.length === 0 ||
    requested.some((name) => typeof name !== "string" || !name.trim()) ||
    new Set(requested).size !== requested.length ||
    (requested.includes("full") && requested.length !== 1)
  )
    throw new Error("Select full OR distinct explicit feature names");
  const full = requested[0] === "full";
  const selected = registry.filter(
    (entry) => full || requested.includes(entry.feature),
  );
  return {
    scope: full ? "full-registered-suite" : "selected-features",
    requested,
    selected: selected.map((entry) => entry.id),
    notSelected: registry
      .filter((entry) => !selected.includes(entry))
      .map((entry) => entry.id),
    missingFeatures: full
      ? []
      : requested.filter(
          (name) => !registry.some((entry) => entry.feature === name),
        ),
    coverageLimits,
  };
}
