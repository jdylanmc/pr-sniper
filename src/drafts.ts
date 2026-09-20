type Control = HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement;
interface Draft {
  key: string;
  values: { name: string; value: string; checked: boolean }[];
}

export function trackDrafts(root: HTMLElement) {
  let revision = 0;
  const changed = (event: Event) => {
    revision++;
    if (!(event.target instanceof HTMLElement)) return;
    const form = event.target.closest<HTMLFormElement>("form[data-draft-key]");
    if (!form) return;
    form.dataset.dirty = "true";
    const group = event.target.closest<HTMLElement>("[data-draft-group]");
    if (group) group.dataset.dirty = "true";
  };
  root.addEventListener("input", changed);
  root.addEventListener("change", changed);
  root.addEventListener("click", () => revision++);
  root.addEventListener("submit", () => revision++);
  return () => revision;
}

export function hasDrafts(root: HTMLElement) {
  return root.querySelector("form[data-dirty]") !== null;
}

export function clearDraft(form: HTMLFormElement | null) {
  if (!form) return;
  delete form.dataset.dirty;
  form.querySelectorAll<HTMLElement>("[data-dirty]").forEach((group) => {
    delete group.dataset.dirty;
  });
}

export function captureDrafts(root: HTMLElement): Draft[] {
  return [...root.querySelectorAll<HTMLFormElement>("form[data-dirty]")].map(
    (form) => {
      const groups = form.querySelectorAll<HTMLElement>("[data-draft-group]");
      const scopes = groups.length
        ? [...groups].filter((group) => group.dataset.dirty)
        : [form];
      const controls = scopes.flatMap((scope) => {
        const override =
          scope.querySelector<HTMLInputElement>("[data-override]");
        return override && !override.checked
          ? [override]
          : [...scope.querySelectorAll<Control>("input,select,textarea")];
      });
      return {
        key: form.dataset.draftKey!,
        values: controls.map((control) => ({
          name: control.name,
          value: control.value,
          checked: control instanceof HTMLInputElement && control.checked,
        })),
      };
    },
  );
}

export function restoreDrafts(root: HTMLElement, drafts: Draft[]) {
  const findForm = (key: string) =>
    [...root.querySelectorAll<HTMLFormElement>("form[data-draft-key]")].find(
      (form) => form.dataset.draftKey === key,
    );
  for (const draft of drafts) {
    if (!findForm(draft.key)) {
      [...root.querySelectorAll<HTMLButtonElement>("[data-open-draft]")]
        .find((button) => button.dataset.openDraft === draft.key)
        ?.click();
    }
    const form = findForm(draft.key);
    if (!form) continue;
    for (const saved of draft.values) {
      const control = form.elements.namedItem(saved.name);
      if (
        control instanceof HTMLInputElement ||
        control instanceof HTMLSelectElement ||
        control instanceof HTMLTextAreaElement
      ) {
        control.value = saved.value;
        if (control instanceof HTMLInputElement)
          control.checked = saved.checked;
        control.dispatchEvent(new Event("change", { bubbles: true }));
      }
    }
  }
}

export function lockSettings(editor: HTMLElement) {
  // Serialize saves so one refresh cannot detach another submitting editor.
  const root = editor.closest<HTMLElement>("#content")!;
  const controls = [
    ...root.querySelectorAll<Control | HTMLButtonElement>(
      "input,select,textarea,button",
    ),
  ].map((control) => ({ control, disabled: control.disabled }));
  root.dataset.saving = "true";
  controls.forEach(({ control }) => (control.disabled = true));
  return () => {
    controls.forEach(({ control, disabled }) => (control.disabled = disabled));
    delete root.dataset.saving;
  };
}
