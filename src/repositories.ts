import { invoke } from "@tauri-apps/api/core";

export interface Repository {
  id: string;
  provider: "github";
  name: string;
  enabled: boolean;
}

export function renderRepositories(
  root: HTMLElement,
  repositories: Repository[] | null,
  reload: () => Promise<void>,
  showError: (message: string) => void,
) {
  root.innerHTML = `
    <h2>Watched repositories</h2>
    <p>Saved configuration only. GitHub access and monitoring have not been verified or started.
    No application-defined repository limit; local storage failures are reported here.
    Provider rate limits will be available when GitHub is connected.</p>
    <form id="add-repository">
      <label for="repository">GitHub repository</label>
      <input id="repository" type="text" placeholder="owner/repository or https://github.com/owner/repository" required />
      <button type="submit">Add repository</button>
    </form>
    <div id="repositories"></div>`;

  async function save(command: string, args: Record<string, unknown>) {
    const controls = root.querySelectorAll<
      HTMLInputElement | HTMLButtonElement
    >("input,button");
    controls.forEach((control) => (control.disabled = true));
    try {
      await invoke(command, args);
      await reload();
    } catch (cause) {
      showError(
        typeof cause === "string"
          ? cause
          : "Could not save repository settings. Check local storage permissions.",
      );
      controls.forEach((control) => (control.disabled = false));
    }
  }

  const input = root.querySelector<HTMLInputElement>("#repository")!;
  root.querySelector("form")!.addEventListener("submit", (event) => {
    event.preventDefault();
    void save("save_repository", { repository: input.value });
  });
  if (repositories === null) {
    root
      .querySelectorAll<HTMLInputElement | HTMLButtonElement>("input,button")
      .forEach((control) => (control.disabled = true));
    return;
  }

  for (const repository of repositories) {
    const card = document.createElement("article");
    card.setAttribute("aria-label", repository.name);
    card.innerHTML = `
      <h3></h3>
      <p class="repository-state"></p>
      <div class="actions">
        <button type="button" class="edit">Edit</button>
        <button type="button" class="toggle"></button>
        <button type="button" class="remove">Remove</button>
      </div>
      <div class="editor"></div>`;
    card.querySelector("h3")!.textContent = repository.name;
    card.querySelector(".repository-state")!.textContent = repository.enabled
      ? "Enabled configuration (not monitoring)"
      : "Disabled";
    const editor = card.querySelector<HTMLElement>(".editor")!;
    const toggle = card.querySelector<HTMLButtonElement>(".toggle")!;
    toggle.textContent = repository.enabled ? "Disable" : "Re-enable";
    toggle.addEventListener(
      "click",
      () =>
        void save("update_repository", {
          id: repository.id,
          repository: repository.name,
          enabled: !repository.enabled,
        }),
    );
    card.querySelector(".edit")!.addEventListener("click", () => {
      editor.innerHTML = `
        <form>
          <label>Repository name <input type="text" required /></label>
          <button type="submit">Save repository</button>
          <button type="button" class="cancel">Cancel</button>
        </form>`;
      const name = editor.querySelector("input")!;
      name.value = repository.name;
      editor.querySelector("form")!.addEventListener("submit", (event) => {
        event.preventDefault();
        void save("update_repository", {
          id: repository.id,
          repository: name.value,
          enabled: repository.enabled,
        });
      });
      editor
        .querySelector(".cancel")!
        .addEventListener("click", () => editor.replaceChildren());
      name.focus();
    });
    card.querySelector(".remove")!.addEventListener("click", () => {
      editor.innerHTML = `<p>Remove this repository and its configuration? Later work must recheck current settings before taking action.</p>
        <button type="button" class="confirm">Confirm removal</button>
        <button type="button" class="cancel">Cancel</button>`;
      editor
        .querySelector(".confirm")!
        .addEventListener(
          "click",
          () => void save("remove_repository", { id: repository.id }),
        );
      editor
        .querySelector(".cancel")!
        .addEventListener("click", () => editor.replaceChildren());
    });
    root.querySelector("#repositories")!.append(card);
  }
}
