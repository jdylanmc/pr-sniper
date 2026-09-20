import { invoke } from "@tauri-apps/api/core";
import { renderConnection } from "./connections";
import { clearDraft, lockSettings } from "./drafts";
import {
  renderPolicyForm,
  renderPolicySummary,
  type Policy,
  type PolicyOverrides,
} from "./policy";

export interface Repository {
  id: string;
  provider: "github";
  name: string;
  enabled: boolean;
  overrides?: PolicyOverrides;
}

export function renderRepositories(
  root: HTMLElement,
  repositories: Repository[] | null,
  defaults: Policy | null,
  reload: (warning?: string | null) => Promise<void>,
  showError: (message: string) => void,
) {
  root.innerHTML = `
    <h2>Watched repositories</h2>
    <p>Configuration changes do not start monitoring. Verify GitHub access explicitly for each saved repository.
    No application-defined repository limit; local storage failures are reported here.
    Provider read failures and rate limits appear beside the connection.</p>
    <form id="add-repository" data-draft-key="add">
      <label for="repository">GitHub repository</label>
      <input id="repository" name="repository" type="text" placeholder="owner/repository or https://github.com/owner/repository" required />
      <button type="submit">Add repository</button>
    </form>
    <div id="repositories"></div>`;

  async function save(
    command: string,
    args: Record<string, unknown>,
    form: HTMLFormElement | null = null,
  ) {
    const unlock = lockSettings(root);
    try {
      const result = await invoke<{ warning: string | null }>(command, args);
      clearDraft(form);
      await reload(result.warning);
    } catch (cause) {
      showError(
        typeof cause === "string"
          ? cause
          : "Could not save repository settings. Check local storage permissions.",
      );
    } finally {
      unlock();
    }
  }

  const input = root.querySelector<HTMLInputElement>("#repository")!;
  root.querySelector("form")!.addEventListener("submit", (event) => {
    event.preventDefault();
    void save("save_repository", { repository: input.value }, input.form);
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
        <button type="button" class="policy">Policy</button>
        <button type="button" class="toggle"></button>
        <button type="button" class="remove">Remove</button>
      </div>
      <div class="policy-summary"></div>
      <section class="connection"></section>
      <div class="editor"></div>`;
    card.querySelector("h3")!.textContent = repository.name;
    renderConnection(card.querySelector(".connection")!, repository);
    card.querySelector(".repository-state")!.textContent = repository.enabled
      ? "Enabled configuration (not monitoring)"
      : "Disabled";
    const editor = card.querySelector<HTMLElement>(".editor")!;
    card.querySelector<HTMLButtonElement>(".policy")!.dataset.openDraft =
      `policy:${repository.id}`;
    card.querySelector<HTMLButtonElement>(".edit")!.dataset.openDraft =
      `repository:${repository.id}`;
    if (defaults) {
      renderPolicySummary(
        card.querySelector(".policy-summary")!,
        defaults,
        repository.overrides ?? {},
      );
      card
        .querySelector(".policy")!
        .addEventListener("click", () =>
          renderPolicyForm(editor, defaults, repository, reload, showError),
        );
    }
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
          <label>Repository name <input name="repository" type="text" required /></label>
          <button type="submit">Save repository</button>
          <button type="button" class="cancel">Cancel</button>
        </form>`;
      const name = editor.querySelector("input")!;
      name.value = repository.name;
      name.form!.dataset.draftKey = `repository:${repository.id}`;
      editor.querySelector("form")!.addEventListener("submit", (event) => {
        event.preventDefault();
        void save(
          "update_repository",
          {
            id: repository.id,
            repository: name.value,
            enabled: repository.enabled,
          },
          name.form,
        );
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
