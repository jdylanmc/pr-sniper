export type KeyValue = [
  label: string,
  value: string | HTMLElement,
  copy?: boolean,
];

// Terse key/value grid. Empty values are skipped; copy values copy on tap.
export function keyValues(root: HTMLElement, rows: KeyValue[]) {
  const list = document.createElement("dl");
  list.className = "context-list queue-facts";
  for (const [label, value, copy] of rows) {
    if (value === "") continue;
    const term = document.createElement("dt");
    term.textContent = label;
    const description = document.createElement("dd");
    if (typeof value !== "string") description.append(value);
    else if (copy) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "copy-value";
      button.tabIndex = -1;
      button.title = `Copy ${label}`;
      button.setAttribute("aria-label", `Copy ${label}`);
      button.textContent = value;
      button.onclick = () => {
        void navigator.clipboard?.writeText(value).catch(() => undefined);
        button.dataset.copied = "";
        setTimeout(() => delete button.dataset.copied, 1200);
      };
      description.append(button);
    } else description.textContent = value;
    list.append(term, description);
  }
  root.append(list);
  return list;
}
