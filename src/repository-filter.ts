import type { QueueItem } from "./queue";

type Job = Pick<
  QueueItem["job"],
  | "provider"
  | "account_id"
  | "account_login"
  | "repository_id"
  | "repository_name"
>;

export interface RepositoryEntry {
  key: string;
  provider: string;
  account_id: string;
  account_login: string;
  name: string;
  label: string;
  count: number;
}

// Provider Repository Identity plus its bound account: one row per binding.
export const repositoryKey = (job: Job) =>
  JSON.stringify([job.provider, job.account_id, job.repository_id]);

const shortName = (name: string) => {
  const parts = name.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? name;
};

/** Repositories with rows, each with its count, grouped by provider and
 * account. Labels are short names; a short name shared by two repositories
 * falls back to the full name. */
export function repositoryEntries(jobs: Job[]): RepositoryEntry[] {
  const entries = new Map<string, RepositoryEntry>();
  for (const job of jobs) {
    const key = repositoryKey(job);
    const entry = entries.get(key);
    if (entry) entry.count++;
    else
      entries.set(key, {
        key,
        provider: job.provider,
        account_id: job.account_id,
        account_login: job.account_login,
        name: job.repository_name,
        label: shortName(job.repository_name),
        count: 1,
      });
  }
  const list = [...entries.values()];
  const shared = new Map<string, number>();
  for (const entry of list)
    shared.set(entry.label, (shared.get(entry.label) ?? 0) + 1);
  for (const entry of list)
    if ((shared.get(entry.label) ?? 0) > 1) entry.label = entry.name;
  const order = (entry: RepositoryEntry) =>
    [
      entry.provider === "github" ? "0" : "1",
      entry.account_login.toLowerCase(),
      entry.account_id,
      entry.label.toLowerCase(),
      entry.name,
    ].join("\u0000");
  return list.sort((a, b) => (order(a) < order(b) ? -1 : 1));
}

/** Drops selected repositories that no longer have rows. */
export function prune(selection: Set<string>, entries: RepositoryEntry[]) {
  const present = new Set(entries.map((entry) => entry.key));
  for (const key of selection) if (!present.has(key)) selection.delete(key);
}

export function selectionLabel(
  selection: ReadonlySet<string>,
  entries: RepositoryEntry[],
) {
  if (!selection.size) return "All repos";
  if (selection.size === 1)
    return entries.find((entry) => selection.has(entry.key))?.label ?? "1 repo";
  return `${selection.size} repos`;
}

export const shows = (selection: ReadonlySet<string>, job: Job) =>
  !selection.size || selection.has(repositoryKey(job));

const svg = (path: string) =>
  `<svg class="panel-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="${path}"/></svg>`;
const repositoryIcon = "M3 6h7l2 3h9v11H3ZM3 6V4h7l2 2h7v3";
const checkIcon = "m6 12 4 4 8-8";
const chevron = "m9 5 7 7-7 7";

/** The drill-in opener: a chip showing the current selection. */
export function filterChip(
  list: string,
  selection: ReadonlySet<string>,
  entries: RepositoryEntry[],
) {
  const chip = document.createElement("button");
  chip.type = "button";
  chip.className = "filter-chip repository-filter-chip";
  chip.dataset.repositoryFilterOpen = list;
  const label = selectionLabel(selection, entries);
  chip.setAttribute("aria-pressed", String(selection.size > 0));
  chip.setAttribute("aria-label", `Repositories: ${label}`);
  chip.title = label;
  chip.innerHTML = `${svg(repositoryIcon)}<span class="repository-filter-label"></span>${svg(chevron)}`;
  chip.querySelector("span")!.textContent = label;
  return chip;
}

/** The drill-in: All, then one pressed-toggle row per repository, grouped by
 * provider account. Focus stays on the toggled row across redraws. */
export function renderPicker(
  root: HTMLElement,
  selection: Set<string>,
  entries: RepositoryEntry[],
  changed: () => void,
) {
  const active =
    document.activeElement instanceof HTMLElement &&
    root.contains(document.activeElement)
      ? (document.activeElement.dataset.repositoryFilter ??
        (document.activeElement.hasAttribute("data-repository-filter-all")
          ? ""
          : undefined))
      : undefined;
  root.replaceChildren();
  const all = document.createElement("button");
  all.type = "button";
  all.className = "repository-filter-row repository-filter-all";
  all.dataset.repositoryFilterAll = "";
  all.setAttribute("aria-pressed", String(!selection.size));
  const total = entries.reduce((sum, entry) => sum + entry.count, 0);
  all.setAttribute("aria-label", `All repos ${total}`);
  all.innerHTML = `<span class="repository-filter-check">${svg(checkIcon)}</span><strong>All repos</strong><span class="repository-filter-count" aria-hidden="true">${total}</span>`;
  all.onclick = () => {
    selection.clear();
    changed();
  };
  const top = document.createElement("div");
  top.className = "repository-filter-group";
  top.append(all);
  root.append(top);
  let group: HTMLElement | undefined;
  let account = "";
  entries.forEach((entry, index) => {
    const owner = JSON.stringify([entry.provider, entry.account_id]);
    if (!group || owner !== account) {
      account = owner;
      group = document.createElement("section");
      group.className = "repository-filter-group";
      group.dataset.provider = entry.provider;
      const heading = document.createElement("h2");
      heading.id = `repository-filter-account-${index}`;
      const mark = document.createElement("span");
      mark.className = "provider-mark";
      mark.dataset.brand =
        entry.provider === "github" ? "github" : "azure-devops";
      mark.setAttribute("aria-hidden", "true");
      mark.textContent = entry.provider === "github" ? "GH" : "AZ";
      const name = document.createElement("span");
      name.textContent = entry.account_login;
      heading.append(mark, name);
      group.setAttribute("aria-labelledby", heading.id);
      group.append(heading);
      root.append(group);
    }
    const row = document.createElement("button");
    row.type = "button";
    row.className = "repository-filter-row";
    row.dataset.repositoryFilter = entry.key;
    row.setAttribute("aria-pressed", String(selection.has(entry.key)));
    row.setAttribute("aria-label", `${entry.label} ${entry.count}`);
    row.title = entry.name;
    row.innerHTML = `<span class="repository-filter-check">${svg(checkIcon)}</span><strong></strong><span class="repository-filter-count" aria-hidden="true">${entry.count}</span>`;
    row.querySelector("strong")!.textContent = entry.label;
    row.onclick = () => {
      if (!selection.delete(entry.key)) selection.add(entry.key);
      changed();
    };
    group.append(row);
  });
  if (active === undefined) return;
  const target =
    active === ""
      ? root.querySelector<HTMLElement>("[data-repository-filter-all]")
      : [
          ...root.querySelectorAll<HTMLElement>("[data-repository-filter]"),
        ].find((row) => row.dataset.repositoryFilter === active);
  (target ?? all).focus({ preventScroll: true });
}

/** Shows the chip in `slot` while there is a choice to make, updating it in
 * place so a focused chip keeps focus across redraws. */
export function syncChip(
  slot: HTMLElement,
  list: string,
  selection: ReadonlySet<string>,
  entries: RepositoryEntry[],
  open: (opener: HTMLElement) => void,
) {
  const shown = entries.length > 1 || selection.size > 0;
  slot.hidden = !shown;
  const existing = slot.querySelector<HTMLButtonElement>(
    "[data-repository-filter-open]",
  );
  if (!shown) {
    existing?.remove();
    return;
  }
  const next = filterChip(list, selection, entries);
  next.onclick = () => open(next);
  if (!existing) {
    slot.prepend(next);
    return;
  }
  for (const name of ["aria-pressed", "aria-label", "title"])
    existing.setAttribute(name, next.getAttribute(name)!);
  existing.querySelector(".repository-filter-label")!.textContent =
    next.querySelector(".repository-filter-label")!.textContent;
  existing.onclick = () => open(existing);
}
