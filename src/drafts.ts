type Control = HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement;

export function lockSettings(editor: HTMLElement) {
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
