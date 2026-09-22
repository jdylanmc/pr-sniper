import { invoke } from "@tauri-apps/api/core";
import { renderConnection } from "./connections";
import {
  effectivePolicy,
  type Policy,
  type PolicyOverrides,
  type Selector,
} from "./policy";
import type { Repository } from "./repositories";
import crosshair from "./crosshair.svg";
import "./settings.css";

interface Preset {
  id: string;
  name: string;
  body: string;
}
interface Settings {
  launch_at_login: boolean;
  defaults: Policy;
  repositories?: (Repository & { review_preset?: string })[];
  root_folder?: string;
  presets?: Preset[];
  default_review_preset?: string;
}
interface Snapshot {
  settings: Settings | null;
  settings_persisted: boolean;
  isolated: boolean;
  login_registration: "absent" | "registered" | "invalid" | null;
  error: string | null;
}
interface Discovered {
  path: string;
  name: string | null;
  unavailable: string | null;
}
interface Discovery {
  root: string;
  repositories: Discovered[];
  warnings: string[];
}
type Section = "repositories" | "people" | "reviews" | "automation" | "presets";
type ConfiguredRepository = NonNullable<Settings["repositories"]>[number];
const sections: Record<Section, [string, string]> = {
  repositories: [
    "Repositories",
    "Choose where PR Sniper looks for pull requests.",
  ],
  people: ["People", "Follow the people whose work you want to review."],
  reviews: ["Review defaults", "Set the starting point for every repository."],
  automation: [
    "Automation",
    "Keep reviews and publication under your control.",
  ],
  presets: ["Review presets", "Give your reviews a reusable point of view."],
};
const escape = (value: string) =>
  value.replace(
    /[&<>"']/g,
    (c) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[c]!,
  );
const clone = <T>(value: T): T => structuredClone(value);
const localZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone;
const option = (value: string, label: string, selected: string) =>
  `<option value="${escape(value)}" ${value === selected ? "selected" : ""}>${escape(label)}</option>`;
const reason = (error: unknown) => {
  const errors: Record<string, string> = {
    missing_cli:
      "GitHub CLI is missing. Install the official gh CLI, sign in, then try again.",
    signed_out:
      "GitHub is disconnected. Run gh auth login in your terminal, then try again.",
    broken_cli:
      "GitHub CLI could not run. Repair your gh installation, then try again.",
    missing_read_permission:
      "This person is unavailable. Check the GitHub login and your account access.",
    rate_limited: "GitHub rate limited this lookup. Wait before trying again.",
    network: "Cannot reach GitHub. Check your network and try again.",
    timeout: "GitHub lookup timed out. Try again.",
    invalid_response:
      "Enter a valid GitHub login and try again. No identity was added.",
    provider_failure:
      "GitHub lookup failed. Check provider health and try again.",
  };
  return typeof error === "string"
    ? (errors[error] ?? error)
    : "This action failed. Check local access and try again.";
};

export async function mountSettings(app: HTMLElement) {
  document.body.classList.add("settings-page");
  app.className = "settings-window";
  app.innerHTML = `<aside class="settings-sidebar"><div class="settings-brand"><img src="${crosshair}" alt="" />PR Sniper</div><p class="settings-caption">Preferences</p>
    <nav aria-label="Settings sections">${Object.entries(sections)
      .map(
        ([key, [title]]) =>
          `<button type="button" data-section="${key}">${title}</button>`,
      )
      .join("")}</nav>
    <label class="mobile-section">Section<select aria-label="Settings section">${Object.entries(
      sections,
    )
      .map(([key, [title]]) => option(key, title, "repositories"))
      .join("")}</select></label></aside>
    <div class="settings-main"><header class="settings-heading"><h1 tabindex="-1"></h1><p></p></header>
    <p id="error" role="alert" hidden></p><section id="content"></section>
    <footer class="settings-savebar"><span role="status" id="save-status">Loading settings...</span><button id="reset-settings" disabled>Reset changes</button><button class="primary" id="save-settings" disabled>Save changes</button></footer></div>`;
  const content = app.querySelector<HTMLElement>("#content")!;
  const error = app.querySelector<HTMLElement>("#error")!;
  const status = app.querySelector<HTMLElement>("#save-status")!;
  const save = app.querySelector<HTMLButtonElement>("#save-settings")!;
  const reset = app.querySelector<HTMLButtonElement>("#reset-settings")!;
  let snapshot: Snapshot;
  let saved: Settings;
  let draft: Settings;
  let section: Section = "repositories";
  let discovery: Discovery | null = null;
  let query = "";
  let busy = false;
  let revision = 0;
  const dirty = () =>
    !!draft && JSON.stringify(draft) !== JSON.stringify(saved);
  const showError = (message: string) => {
    error.textContent = message;
    error.hidden = false;
  };
  const clearError = () => {
    error.hidden = true;
    error.textContent = "";
  };
  const repositories = () => draft.repositories ?? [];
  const presets = () => draft.presets ?? [];
  function changed() {
    revision++;
    status.textContent = dirty() ? "Unsaved changes" : "All changes saved";
    save.disabled = !dirty() || busy;
    reset.disabled = !dirty() || busy;
  }
  function updatePolicy<K extends keyof Policy>(
    repository: ConfiguredRepository | undefined,
    key: K,
    value: Policy[K],
  ) {
    if (repository) (repository.overrides ??= {})[key] = value;
    else draft.defaults[key] = value;
    changed();
  }
  const policy = (repository?: ConfiguredRepository) =>
    effectivePolicy(draft.defaults, repository?.overrides ?? {});

  function dialog(title: string, body: string) {
    const modal = document.createElement("dialog");
    modal.className = "settings-dialog";
    modal.setAttribute("aria-label", title);
    modal.innerHTML = `<div class="dialog-head"><h2>${escape(title)}</h2><button type="button" aria-label="Close dialog">Close</button></div><div class="dialog-body">${body}</div>`;
    content.append(modal);
    const opener = document.activeElement;
    modal.querySelector("button")!.onclick = () => modal.close();
    modal.addEventListener("close", () => {
      modal.remove();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    });
    modal.showModal();
    return modal;
  }

  function render() {
    if (!draft) return;
    app.querySelector("h1")!.textContent = sections[section][0];
    app.querySelector(".settings-heading p")!.textContent =
      sections[section][1];
    app
      .querySelectorAll<HTMLButtonElement>("[data-section]")
      .forEach((button) => {
        if (button.dataset.section === section)
          button.setAttribute("aria-current", "page");
        else button.removeAttribute("aria-current");
      });
    app.querySelector<HTMLSelectElement>(".mobile-section select")!.value =
      section;
    content.replaceChildren();
    if (section === "repositories") renderRepositories();
    if (section === "people") renderPeople(content);
    if (section === "reviews") renderReview(content);
    if (section === "automation") renderAutomation(content);
    if (section === "presets") renderPresets();
    changed();
  }
  function navigate(next: Section) {
    if (busy) return;
    section = next;
    render();
  }
  app
    .querySelectorAll<HTMLButtonElement>("[data-section]")
    .forEach((button) => {
      button.onclick = () => navigate(button.dataset.section as Section);
    });
  app.querySelector<HTMLSelectElement>(".mobile-section select")!.onchange = (
    event,
  ) => {
    navigate((event.target as HTMLSelectElement).value as Section);
  };

  async function scan(choose: boolean) {
    clearError();
    busy = true;
    changed();
    try {
      const result = choose
        ? await invoke<Discovery | null>("choose_repository_folder")
        : await invoke<Discovery>("discover_repositories", {
            root: draft.root_folder,
          });
      if (result) {
        discovery = result;
        draft.root_folder = result.root;
      }
    } catch (cause) {
      showError(reason(cause));
    } finally {
      busy = false;
      render();
    }
  }

  function renderRepositories() {
    content.innerHTML = `<div class="folder-card"><div class="folder-symbol" aria-hidden="true">&#128193;</div><div><strong>${escape(draft.root_folder ?? "Choose your repository folder")}</strong><p>${discovery ? `${discovery.repositories.length} local repositories discovered` : "Only a folder you choose is scanned."}</p></div><button id="choose-folder">Choose folder...</button></div>
      <div class="repository-toolbar"><input id="repo-search" type="search" aria-label="Find a repository" placeholder="Find a repository..." value="${escape(query)}" /><button id="select-visible">Select visible</button></div>
      <div class="list-label"><span>Repository</span><span id="selected-count"></span></div><div class="repository-list"></div>
      <p class="settings-hint">Monitoring configuration only. Reviews and comments stay separate. No polling runs in this build.</p>
      <div class="settings-actions"><button id="add-repository">Add repository manually...</button>${draft.root_folder ? '<button id="rescan">Scan chosen folder</button>' : ""}</div>
      ${discovery?.warnings.map((warning) => `<p class="settings-notice">${escape(warning)}</p>`).join("") ?? ""}`;
    content.querySelector<HTMLButtonElement>("#choose-folder")!.onclick =
      () => {
        if (!busy) void scan(true);
      };
    content
      .querySelector<HTMLButtonElement>("#rescan")
      ?.addEventListener("click", () => {
        if (!busy) void scan(false);
      });
    content.querySelector<HTMLInputElement>("#repo-search")!.oninput = (
      event,
    ) => {
      query = (event.target as HTMLInputElement).value;
      rows();
    };
    content.querySelector<HTMLButtonElement>("#add-repository")!.onclick = () =>
      editRepository();
    content.querySelector<HTMLButtonElement>("#select-visible")!.onclick =
      () => {
        for (const item of visible()) if (item.name) select(item.name, true);
        rows();
      };
    function all(): Discovered[] {
      const list = [...(discovery?.repositories ?? [])];
      const names = new Set(list.map((item) => item.name));
      for (const repo of repositories())
        if (!names.has(repo.name))
          list.push({ name: repo.name, path: "", unavailable: null });
      return list;
    }
    function visible() {
      return all().filter((item) =>
        `${item.name ?? ""} ${item.path}`
          .toLowerCase()
          .includes(query.toLowerCase()),
      );
    }
    function select(name: string, enabled: boolean) {
      const existing = repositories().find((r) => r.name === name);
      if (existing) existing.enabled = enabled;
      else if (enabled)
        (draft.repositories ??= []).push({
          id: crypto.randomUUID(),
          name,
          enabled,
          provider: "github",
        });
      changed();
    }
    function rows() {
      const list = content.querySelector<HTMLElement>(".repository-list")!;
      list.replaceChildren();
      content.querySelector("#selected-count")!.textContent =
        `${repositories().filter((r) => r.enabled).length} selected`;
      if (!visible().length) {
        list.innerHTML = `<div class="settings-empty"><strong>${query ? "No matching repositories" : "No repositories yet"}</strong><p>${query ? "Try a repository or organization name." : "Choose a local folder or add a GitHub repository manually."}</p></div>`;
      }
      for (const item of visible()) {
        const repository = repositories().find((r) => r.name === item.name);
        const row = document.createElement("article");
        row.className = "repository-row";
        row.setAttribute(
          "aria-label",
          item.name ?? item.path.split("/").pop() ?? "Unavailable repository",
        );
        row.innerHTML = `<input type="checkbox" aria-label="Monitor ${escape(item.name ?? item.path.split("/").pop() ?? "repository")}" ${repository?.enabled ? "checked" : ""} ${!item.name ? "disabled" : ""} />
          <div class="repository-info"><strong>${escape(item.name?.split("/")[1] ?? item.path.split("/").pop() ?? "")}</strong><p>${escape(item.name ?? item.unavailable ?? "Unavailable")}</p></div>
          ${item.name ? `<span class="repository-note">${Object.keys(repository?.overrides ?? {}).length ? "Custom settings" : "Use defaults"}</span><button class="configure">Settings</button>` : ""}`;
        row.querySelector<HTMLInputElement>("input")!.onchange = (event) => {
          select(item.name!, (event.target as HTMLInputElement).checked);
          content.querySelector("#selected-count")!.textContent =
            `${repositories().filter((r) => r.enabled).length} selected`;
        };
        row
          .querySelector<HTMLButtonElement>(".configure")
          ?.addEventListener("click", () => {
            let repo = repositories().find((r) => r.name === item.name);
            if (!repo) {
              repo = {
                id: crypto.randomUUID(),
                name: item.name!,
                enabled: false,
                provider: "github",
              };
              (draft.repositories ??= []).push(repo);
              changed();
            }
            repositoryDialog(repo);
          });
        list.append(row);
      }
    }
    rows();
  }

  function editRepository(repository?: ConfiguredRepository) {
    const modal = dialog(
      repository ? "Edit repository" : "Add repository",
      `<form><label>GitHub repository<input name="repository" required value="${escape(repository?.name ?? "")}" placeholder="owner/repository or https://github.com/owner/repository" /></label><p class="settings-hint">Adding is a draft until you save changes. It does not start reviews.</p><p role="alert" hidden></p><button class="primary">Use repository</button></form>`,
    );
    modal.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      const input = modal.querySelector<HTMLInputElement>("input")!;
      const button = modal.querySelector<HTMLButtonElement>("form button")!;
      button.disabled = true;
      try {
        const name = await invoke<string>("canonical_repository_name", {
          repository: input.value,
        });
        if (
          repositories().some((r) => r.name === name && r.id !== repository?.id)
        )
          throw "This GitHub repository is already configured.";
        if (repository) repository.name = name;
        else
          (draft.repositories ??= []).push({
            id: crypto.randomUUID(),
            name,
            provider: "github",
            enabled: true,
          });
        modal.close();
        render();
      } catch (cause) {
        const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
        alert.textContent = reason(cause);
        alert.hidden = false;
      } finally {
        button.disabled = false;
      }
    };
  }

  function repositoryDialog(repository: ConfiguredRepository) {
    const modal = dialog(
      `Settings for ${repository.name}`,
      `<p class="settings-hint">Each field inherits independently. Unchecking an override restores the current global default for that field only.</p><div class="repo-policy"></div><details><summary>Repository and connection</summary><div class="settings-actions"><button id="rename-repository">Edit repository</button><button id="remove-repository">Remove repository</button></div><div class="connection"></div></details>`,
    );
    const body = modal.querySelector<HTMLElement>(".repo-policy")!;
    renderReview(body, repository);
    renderPeople(body, repository);
    renderAutomation(body, repository);
    if (
      saved.repositories?.some(
        (r) => r.id === repository.id && r.name === repository.name,
      )
    )
      renderConnection(modal.querySelector(".connection")!, repository);
    else
      modal.querySelector(".connection")!.textContent =
        "Save this repository before verifying its GitHub connection.";
    modal.querySelector<HTMLButtonElement>("#rename-repository")!.onclick =
      () => {
        modal.close();
        editRepository(repository);
      };
    modal.querySelector<HTMLButtonElement>("#remove-repository")!.onclick =
      () => {
        const confirm = dialog(
          "Remove repository?",
          `<p>Remove ${escape(repository.name)} and its overrides from your draft? Nothing is removed until Save changes.</p><button class="primary" id="confirm-remove">Remove from settings</button>`,
        );
        confirm.querySelector<HTMLButtonElement>("#confirm-remove")!.onclick =
          () => {
            draft.repositories = repositories().filter(
              (r) => r.id !== repository.id,
            );
            confirm.close();
            modal.close();
            render();
          };
      };
  }

  function field(
    root: HTMLElement,
    key: keyof Policy,
    title: string,
    repository?: ConfiguredRepository,
  ) {
    const group = document.createElement("div");
    group.className = "settings-group";
    group.innerHTML = `${repository ? `<label class="override-toggle"><input type="checkbox" aria-label="Override ${escape(title.toLowerCase())}" ${repository.overrides?.[key] != null ? "checked" : ""} />Override ${escape(title.toLowerCase())}</label>` : ""}
      <fieldset><legend>${escape(title)}</legend></fieldset>`;
    const fieldset = group.querySelector("fieldset")!;
    if (repository) {
      fieldset.disabled = repository.overrides?.[key] == null;
      group.querySelector<HTMLInputElement>("input")!.onchange = (event) => {
        const on = (event.target as HTMLInputElement).checked;
        if (on) updatePolicy(repository, key, clone(policy(repository)[key]));
        else {
          delete repository.overrides?.[key];
          if (key === "prompt") delete repository.review_preset;
          changed();
        }
        const modal = group.closest("dialog");
        if (modal) {
          modal.close();
          repositoryDialog(repository);
        }
      };
    }
    root.append(group);
    return fieldset;
  }

  function renderPeople(root: HTMLElement, repository?: ConfiguredRepository) {
    const target = field(
      root,
      "watched_authors",
      repository ? "People" : "People you watch",
      repository,
    );
    const people = policy(repository).watched_authors;
    target.innerHTML = `<div class="people-list">${people.length ? people.map((person, i) => `<div class="person-row"><span class="person-avatar">${escape(person.login.slice(0, 2).toUpperCase())}</span><div><strong>${escape(person.login)}</strong><p>@${escape(person.login)}</p></div><button type="button" data-remove="${i}" aria-label="Remove ${escape(person.login)}">Remove</button></div>`).join("") : '<p class="settings-empty">No people added yet. Add a GitHub login to follow their pull requests.</p>'}</div>
      <form class="person-lookup"><label>GitHub login<input name="login" placeholder="octocat" autocomplete="off" required /></label><button type="submit">Add person</button><p role="alert" hidden></p></form><p class="settings-hint">Looks up the exact login through your current GitHub CLI account and stores its stable identity. Sign in with gh auth login in your terminal if disconnected.</p>`;
    target
      .querySelectorAll<HTMLButtonElement>("[data-remove]")
      .forEach((button) => {
        button.onclick = () => {
          updatePolicy(
            repository,
            "watched_authors",
            people.filter((_, i) => i !== Number(button.dataset.remove)),
          );
          if (repository) {
            target.closest("dialog")!.close();
            repositoryDialog(repository);
          } else render();
        };
      });
    target.querySelector("form")!.onsubmit = async (event) => {
      event.preventDefault();
      const input = target.querySelector<HTMLInputElement>("[name=login]")!;
      const button = target.querySelector<HTMLButtonElement>("form button")!;
      const alert = target.querySelector<HTMLElement>("[role=alert]")!;
      button.disabled = true;
      alert.hidden = true;
      const lookupRevision = revision;
      try {
        const identity = await invoke<Policy["watched_authors"][number]>(
          "resolve_github_person",
          { login: input.value.trim().replace(/^@/, "") },
        );
        if (!target.isConnected || lookupRevision !== revision)
          throw "Settings changed during lookup. Add this person again.";
        if (people.some((p) => p.id === identity.id))
          throw "This person is already in this watchlist.";
        updatePolicy(repository, "watched_authors", [...people, identity]);
        if (repository) {
          target.closest("dialog")!.close();
          repositoryDialog(repository);
        } else render();
      } catch (cause) {
        alert.textContent = reason(cause);
        alert.hidden = false;
      } finally {
        button.disabled = false;
      }
    };
  }

  function renderReview(root: HTMLElement, repository?: ConfiguredRepository) {
    if (!repository) {
      const notice = document.createElement("div");
      notice.className = "settings-notice";
      notice.innerHTML =
        "<strong>GitHub Copilot CLI</strong><p>Agent health and installed models are not available in this build. Install and sign in to Copilot in your terminal. Setup Doctor execution arrives separately; no successful check is implied.</p>";
      root.append(notice);
    }
    const current = policy(repository);
    const model = field(root, "selector", "Model", repository);
    const selectors: Selector[] = [{ kind: "default" }];
    for (const selection of [
      draft.defaults.selector,
      ...repositories().map((r) => r.overrides?.selector),
    ].filter((s): s is Selector => !!s)) {
      if (
        !selectors.some((s) => JSON.stringify(s) === JSON.stringify(selection))
      )
        selectors.push(selection);
    }
    const selected = selectors.findIndex(
      (s) => JSON.stringify(s) === JSON.stringify(current.selector),
    );
    model.innerHTML = `<label class="setting-row"><span>Model<small>Default lets the review agent choose. Saved selections are retained, not verified.</small></span><select aria-label="Model">${selectors.map((s, i) => option(String(i), s.kind === "default" ? "Default" : `${s.value} (saved ${s.kind}, unverified)`, String(selected))).join("")}</select></label>`;
    model.querySelector("select")!.onchange = (event) =>
      updatePolicy(
        repository,
        "selector",
        clone(selectors[Number((event.target as HTMLSelectElement).value)]),
      );
    const prompt = field(root, "prompt", "Review instructions", repository);
    const presetId = repository
      ? repository.review_preset
      : draft.default_review_preset;
    prompt.innerHTML = `<label>Review preset<select aria-label="Review preset">${option("", "Custom instructions", presetId ?? "")}${presets()
      .map((p) => option(p.id, p.name, presetId ?? ""))
      .join(
        "",
      )}</select></label><label>Review prompt<textarea aria-label="Review prompt" rows="5">${escape(current.prompt)}</textarea></label><p class="settings-hint">Plain text only. Never include credentials or tokens. Editing instructions switches this field to Custom instructions.</p>`;
    const textarea = prompt.querySelector("textarea")!;
    const select = prompt.querySelector("select")!;
    select.onchange = () => {
      if (repository) {
        if (select.value) repository.review_preset = select.value;
        else delete repository.review_preset;
      } else {
        if (select.value) draft.default_review_preset = select.value;
        else delete draft.default_review_preset;
      }
      const preset = presets().find((p) => p.id === select.value);
      if (preset) {
        textarea.value = preset.body;
        updatePolicy(repository, "prompt", preset.body);
      }
      changed();
    };
    textarea.oninput = () => {
      if (repository) delete repository.review_preset;
      else delete draft.default_review_preset;
      select.value = "";
      updatePolicy(repository, "prompt", textarea.value);
    };
  }

  function renderAutomation(
    root: HTMLElement,
    repository?: ConfiguredRepository,
  ) {
    const current = policy(repository);
    for (const [key, title, hint] of [
      [
        "automatic_agent_start",
        "Run reviews automatically",
        "Start analysis when an eligible pull request arrives.",
      ],
      [
        "automatic_comment_publication",
        "Post review comments automatically",
        "Publish findings after analysis. Separate from starting a review.",
      ],
    ] as const) {
      const target = field(root, key, title, repository);
      target.innerHTML = `<label class="setting-row"><span>${title}<small>${hint}</small></span><input type="checkbox" role="switch" aria-label="${title}" ${current[key] ? "checked" : ""} /></label>`;
      target.querySelector("input")!.onchange = (event) =>
        updatePolicy(
          repository,
          key,
          (event.target as HTMLInputElement).checked,
        );
    }
    const schedule = field(root, "schedule", "Schedule", repository);
    const existing = current.schedule;
    const local = localZone();
    const zones = [
      ...new Set([
        local,
        existing.timezone,
        "UTC",
        ...Intl.supportedValuesOf("timeZone"),
      ]),
    ];
    schedule.innerHTML = `<label class="setting-row"><span>Check for pull requests<small>Time zone: ${escape(existing.timezone)}${existing.timezone === local ? " (system local)" : " (saved)"}. Scheduling is not running in this build.</small></span><select aria-label="Check frequency">${[
      5,
      15,
      30,
      60,
      ...(existing.kind === "interval" ? [existing.minutes] : []),
    ]
      .filter((n, i, a) => a.indexOf(n) === i)
      .map((n) =>
        option(
          String(n),
          `Every ${n} minutes`,
          existing.kind === "interval" ? String(existing.minutes) : "cron",
        ),
      )
      .join(
        "",
      )}${option("cron", "Custom schedule (cron)", existing.kind === "cron" ? "cron" : "")}</select></label>
      <details ${existing.kind === "cron" ? "open" : ""}><summary>Advanced scheduling</summary><label>Interval minutes<input name="minutes" type="number" min="1" step="1" value="${existing.kind === "interval" ? existing.minutes : 15}" /></label><label>Cron expression<input name="cron" value="${escape(existing.kind === "cron" ? existing.expression : "0 9 * * MON-FRI")}" /></label><label>Time zone<select name="timezone" aria-label="Time zone">${zones.map((zone) => option(zone, zone === local ? `Local - ${zone}` : zone, existing.timezone)).join("")}</select></label><p class="settings-hint">Cron uses five fields: minute, hour, day, month, weekday. Existing schedules and zones are preserved unless you change them.</p></details>`;
    const frequency = schedule.querySelector<HTMLSelectElement>(
      "[aria-label='Check frequency']",
    )!;
    const minutes = schedule.querySelector<HTMLInputElement>("[name=minutes]")!;
    const cron = schedule.querySelector<HTMLInputElement>("[name=cron]")!;
    const timezone =
      schedule.querySelector<HTMLSelectElement>("[name=timezone]")!;
    const update = () => {
      minutes.disabled = frequency.value === "cron";
      cron.disabled = frequency.value !== "cron";
      updatePolicy(
        repository,
        "schedule",
        frequency.value === "cron"
          ? { kind: "cron", expression: cron.value, timezone: timezone.value }
          : {
              kind: "interval",
              minutes: Number(minutes.value),
              timezone: timezone.value,
            },
      );
    };
    minutes.disabled = existing.kind === "cron";
    cron.disabled = existing.kind !== "cron";
    frequency.onchange = () => {
      if (frequency.value !== "cron") minutes.value = frequency.value;
      else schedule.querySelector("details")!.open = true;
      update();
    };
    minutes.oninput = update;
    cron.oninput = update;
    timezone.onchange = update;
    if (!repository) {
      const details = document.createElement("details");
      details.innerHTML = `<summary>Startup and diagnostics</summary><label><input id="login" type="checkbox" ${draft.launch_at_login ? "checked" : ""} ${snapshot.isolated || snapshot.login_registration === null ? "disabled" : ""} />Request launch at login</label><p class="settings-hint">${snapshot.isolated ? "Isolated development run: changing macOS login items is disabled." : `Saved request, not effective macOS state. Registration: ${snapshot.login_registration ?? "unavailable"}. Startup changes are saved separately and immediately.`}</p><button id="diagnostics">Open redacted diagnostics</button>`;
      root.append(details);
      details.querySelector<HTMLInputElement>("#login")!.onchange = async (
        event,
      ) => {
        const checkbox = event.target as HTMLInputElement;
        checkbox.disabled = true;
        try {
          await invoke("save_login", { enabled: checkbox.checked });
        } catch {
          showError(
            "Startup preference change could not complete. Check macOS Login Items and the saved request.",
          );
        } finally {
          try {
            const fresh = await invoke<Snapshot>("snapshot");
            if (fresh.settings) {
              draft.launch_at_login = fresh.settings.launch_at_login;
              saved.launch_at_login = fresh.settings.launch_at_login;
            }
            snapshot = fresh;
          } catch {
            showError(
              "Could not reload startup registration. Check macOS Login Items.",
            );
          }
          render();
        }
      };
      details.querySelector<HTMLButtonElement>("#diagnostics")!.onclick =
        async () => {
          try {
            await invoke("open_diagnostics");
          } catch {
            showError("Could not open diagnostics.");
          }
        };
    }
  }

  function renderPresets() {
    content.innerHTML = `<div class="section-actions"><h2>Local presets</h2><button id="import-preset">Import...</button><button class="primary" id="new-preset">New preset</button></div><p class="settings-hint">Write once. Fine-tune for each repository. Presets stay on this machine.</p><div class="preset-list"></div>`;
    const list = content.querySelector(".preset-list")!;
    if (!presets().length)
      list.innerHTML =
        '<p class="settings-empty">No named presets yet. Your existing custom review instructions are unchanged.</p>';
    for (const preset of presets()) {
      const row = document.createElement("div");
      row.className = "preset-row";
      row.innerHTML = `<div><h3>${escape(preset.name)}${draft.default_review_preset === preset.id ? ' <span class="repository-note">Default</span>' : ""}</h3><p>${escape(preset.body)}</p></div><button>Edit</button>`;
      row.querySelector("button")!.onclick = () => editPreset(preset);
      list.append(row);
    }
    content.querySelector<HTMLButtonElement>("#new-preset")!.onclick = () =>
      editPreset();
    content.querySelector<HTMLButtonElement>("#import-preset")!.onclick =
      () => {
        const modal = dialog(
          "Import a preset",
          `<form><p>Import plain JSON with only name and body text fields. No commands are executed.</p><label>Preset JSON<textarea name="json" rows="7" placeholder='{"name":"API review","body":"Find breaking API changes."}'></textarea></label><label>Or choose a JSON file<input type="file" accept=".json,application/json" /></label><p class="settings-hint">Maximum 24 KB. Name: 1-80 characters. Instructions: 1-12,000 characters.</p><p role="alert" hidden></p><button class="primary">Import preset</button></form>`,
        );
        const text = modal.querySelector("textarea")!;
        const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
        const fail = (message: string) => {
          alert.textContent = message;
          alert.hidden = false;
        };
        modal.querySelector<HTMLInputElement>("[type=file]")!.onchange = async (
          event,
        ) => {
          const file = (event.target as HTMLInputElement).files?.[0];
          if (!file) return;
          if (file.size > 24576) {
            fail("Choose a JSON file smaller than 24 KB.");
            return;
          }
          try {
            text.value = await file.text();
            alert.hidden = true;
          } catch {
            fail("Cannot read this file. Paste the JSON instead.");
          }
        };
        modal.querySelector("form")!.onsubmit = (event) => {
          event.preventDefault();
          try {
            if (new TextEncoder().encode(text.value).length > 24576)
              throw "Use a JSON preset smaller than 24 KB.";
            let value: unknown;
            try {
              value = JSON.parse(text.value);
            } catch {
              throw "Invalid JSON. Use an object with name and body text fields.";
            }
            if (
              !value ||
              typeof value !== "object" ||
              Array.isArray(value) ||
              Object.keys(value).some((k) => k !== "name" && k !== "body") ||
              !("name" in value) ||
              !("body" in value) ||
              typeof value.name !== "string" ||
              typeof value.body !== "string"
            )
              throw "Use only name and body text fields.";
            addPreset(value.name, value.body);
            modal.close();
            render();
          } catch (cause) {
            fail(reason(cause));
          }
        };
      };
  }
  function addPreset(name: string, body: string, existing?: Preset) {
    if (
      !name.trim() ||
      [...name.trim()].length > 80 ||
      !body.trim() ||
      [...body].length > 12000
    )
      throw "Use a name (1-80 characters) and instructions (1-12,000 characters).";
    if (
      /gh[pousr]_|github_pat_|-----BEGIN (?:RSA )?PRIVATE KEY/.test(name + body)
    )
      throw "Credentials and tokens must not be stored in configuration.";
    if (
      presets().some(
        (p) =>
          p.id !== existing?.id &&
          p.name.toLowerCase() === name.trim().toLowerCase(),
      )
    )
      throw "Choose a unique preset name.";
    const preset = existing ?? { id: crypto.randomUUID(), name: "", body: "" };
    preset.name = name.trim();
    preset.body = body;
    if (!existing) (draft.presets ??= []).push(preset);
    if (draft.default_review_preset === preset.id) draft.defaults.prompt = body;
    for (const repository of repositories())
      if (repository.review_preset === preset.id)
        (repository.overrides ??= {}).prompt = body;
    changed();
  }
  function editPreset(preset?: Preset) {
    const modal = dialog(
      preset ? "Edit preset" : "New review preset",
      `<form><label>Preset name<input name="name" maxlength="80" value="${escape(preset?.name ?? "")}" required /></label><label>Review instructions<textarea name="body" rows="7" maxlength="12000" required>${escape(preset?.body ?? "")}</textarea></label><p class="settings-hint">Plain text only. Never include credentials. Changes remain a draft until Save changes.</p><p role="alert" hidden></p><button class="primary">Save preset</button></form>`,
    );
    modal.querySelector("form")!.onsubmit = (event) => {
      event.preventDefault();
      try {
        addPreset(
          modal.querySelector<HTMLInputElement>("[name=name]")!.value,
          modal.querySelector<HTMLTextAreaElement>("[name=body]")!.value,
          preset,
        );
        modal.close();
        render();
      } catch (cause) {
        const alert = modal.querySelector<HTMLElement>("[role=alert]")!;
        alert.textContent = reason(cause);
        alert.hidden = false;
      }
    };
  }

  save.onclick = async () => {
    if (busy || !dirty()) return;
    clearError();
    busy = true;
    changed();
    const controls = [
      ...app.querySelectorAll<
        | HTMLInputElement
        | HTMLButtonElement
        | HTMLSelectElement
        | HTMLTextAreaElement
      >("input,button,select,textarea"),
    ].map((control) => ({ control, disabled: control.disabled }));
    controls.forEach(({ control }) => {
      control.disabled = true;
    });
    try {
      const result = await invoke<{
        settings: Settings;
        warning: string | null;
      }>("save_preferences", { settings: draft, expected: saved });
      saved = clone(result.settings);
      draft = clone(result.settings);
      if (result.warning) showError(result.warning);
    } catch (cause) {
      showError(reason(cause));
    } finally {
      controls.forEach(({ control, disabled }) => {
        control.disabled = disabled;
      });
      busy = false;
      render();
    }
  };
  reset.onclick = () => {
    if (!busy) {
      draft = clone(saved);
      discovery = null;
      clearError();
      render();
    }
  };
  async function load() {
    const requestRevision = revision;
    try {
      const state = await invoke<Snapshot>("snapshot");
      if (requestRevision !== revision || dirty() || busy) return;
      snapshot = state;
      if (!state.settings) {
        showError(
          state.error ??
            "Settings unavailable. Repair local configuration before saving.",
        );
        return;
      }
      saved = clone(state.settings);
      draft = clone(state.settings);
      if (!state.settings_persisted)
        draft.defaults.schedule.timezone = localZone();
      if (state.error) showError(state.error);
      render();
    } catch {
      showError(
        "Could not read Settings. Open the native application and check local storage access.",
      );
    }
  }
  window.addEventListener("focus", () => {
    if (!dirty() && !busy && !content.querySelector("dialog")) void load();
  });
  await load();
}
