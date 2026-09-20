import { invoke } from "@tauri-apps/api/core";

export type Schedule =
  | { kind: "interval"; minutes: number; timezone: string }
  | { kind: "cron"; expression: string; timezone: string };
export type Selector =
  { kind: "default" } | { kind: "model" | "agent"; value: string };
export interface Policy {
  schedule: Schedule;
  watched_authors: { id: string; login: string }[];
  reviewer_assignment: boolean;
  adapter: "copilot";
  selector: Selector;
  prompt: string;
  automatic_agent_start: boolean;
  automatic_comment_publication: boolean;
}
export type PolicyOverrides = Partial<Policy>;

const fields: { key: keyof Policy; title: string; override: string }[] = [
  { key: "schedule", title: "Schedule", override: "schedule" },
  {
    key: "watched_authors",
    title: "Watched authors",
    override: "watched authors",
  },
  {
    key: "reviewer_assignment",
    title: "Reviewer assignment",
    override: "reviewer assignment",
  },
  { key: "adapter", title: "Agent adapter", override: "adapter" },
  { key: "selector", title: "Selector", override: "selector" },
  { key: "prompt", title: "Review prompt", override: "prompt" },
  {
    key: "automatic_agent_start",
    title: "Automatic agent start",
    override: "automatic agent start",
  },
  {
    key: "automatic_comment_publication",
    title: "Automatic comment publication",
    override: "automatic comment publication",
  },
];

export function effectivePolicy(
  defaults: Policy,
  overrides: PolicyOverrides,
): Policy {
  return {
    schedule: overrides.schedule ?? defaults.schedule,
    watched_authors: overrides.watched_authors ?? defaults.watched_authors,
    reviewer_assignment:
      overrides.reviewer_assignment ?? defaults.reviewer_assignment,
    adapter: overrides.adapter ?? defaults.adapter,
    selector: overrides.selector ?? defaults.selector,
    prompt: overrides.prompt ?? defaults.prompt,
    automatic_agent_start:
      overrides.automatic_agent_start ?? defaults.automatic_agent_start,
    automatic_comment_publication:
      overrides.automatic_comment_publication ??
      defaults.automatic_comment_publication,
  };
}

export function renderPolicySummary(
  root: HTMLElement,
  defaults: Policy,
  overrides: PolicyOverrides,
) {
  const policy = effectivePolicy(defaults, overrides);
  const values: Record<keyof Policy, string> = {
    schedule: `${policy.schedule.kind === "interval" ? `Every ${policy.schedule.minutes} minutes` : policy.schedule.expression} / ${policy.schedule.timezone}`,
    watched_authors:
      policy.watched_authors
        .map(({ id, login }) => `${id}:${login}`)
        .join(", ") || "None",
    reviewer_assignment: policy.reviewer_assignment ? "on" : "off",
    adapter: "GitHub Copilot CLI (not verified)",
    selector:
      policy.selector.kind === "default"
        ? "Adapter default"
        : `${policy.selector.kind}: ${policy.selector.value}`,
    prompt: policy.prompt,
    automatic_agent_start: policy.automatic_agent_start ? "on" : "off",
    automatic_comment_publication: policy.automatic_comment_publication
      ? "on"
      : "off",
  };
  for (const { key, title } of fields) {
    const row = document.createElement("p");
    row.className = "policy-value";
    row.textContent = `${title}: ${values[key]} (${overrides[key] == null ? "Global default" : "Repository override"})`;
    root.append(row);
  }
}

const controls: Record<keyof Policy, string> = {
  schedule: `
    <label>Schedule type <select name="schedule-kind" aria-label="Schedule type"><option value="interval">Fixed interval</option><option value="cron">Five-field cron</option></select></label>
    <label class="interval">Interval minutes <input name="minutes" type="number" min="1" step="1" /></label>
    <label class="cron">Cron expression <input name="expression" type="text" placeholder="0 9 * * MON-FRI" /></label>
    <label>Time zone <input name="timezone" type="text" placeholder="America/New_York" /></label>`,
  watched_authors: `<label>Watched GitHub identities <textarea name="authors" placeholder="12345:octo"></textarea></label>
    <p class="hint">One stable numeric GitHub account ID:login per line. IDs are used for matching; login is a display label. Entries are not verified against GitHub yet.</p>`,
  reviewer_assignment: `<label><input name="reviewer" type="checkbox" /> Reviewer assignment</label>`,
  adapter: `<label>Agent adapter <select name="adapter" aria-label="Agent adapter"><option value="copilot">GitHub Copilot CLI (not verified)</option></select></label>`,
  selector: `<label>Selector type <select name="selector-kind" aria-label="Selector type"><option value="default">Adapter default</option><option value="model">Model</option><option value="agent">Named agent</option></select></label>
    <label class="selector-value">Model or named agent <input name="selector-value" type="text" /></label>`,
  prompt: `<label>Review prompt <textarea name="prompt" rows="4"></textarea></label><p class="hint">Configuration is not secure storage. Never enter credentials or tokens.</p>`,
  automatic_agent_start: `<label><input name="start" type="checkbox" /> Automatic agent start</label>`,
  automatic_comment_publication: `<label><input name="publish" type="checkbox" /> Automatic comment publication</label>`,
};

export function renderPolicyForm(
  root: HTMLElement,
  defaults: Policy,
  repository: { id: string; overrides?: PolicyOverrides } | null,
  reload: () => Promise<void>,
  showError: (message: string) => void,
) {
  const overrides = repository?.overrides ?? {};
  const policy = effectivePolicy(defaults, overrides);
  const form = document.createElement("form");
  form.setAttribute(
    "aria-label",
    repository ? "Repository policy" : "Global defaults",
  );
  form.noValidate = true;
  for (const { key, title, override } of fields) {
    const group = document.createElement("div");
    group.className = "policy-field";
    group.innerHTML = `${repository ? `<label><input type="checkbox" data-override="${key}" /> Override ${override}</label>` : ""}
      <fieldset data-field="${key}"><legend>${title}</legend>${controls[key]}</fieldset>`;
    form.append(group);
  }
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.textContent = repository ? "Save policy" : "Save defaults";
  form.append(submit);
  root.replaceChildren(form);
  const input = (name: string) =>
    form.querySelector<
      HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement
    >(`[name="${name}"]`)!;
  const checkbox = (name: string) =>
    form.querySelector<HTMLInputElement>(`input[name="${name}"]`)!;
  const value = (name: string) => input(name).value;
  const overrideControl = (key: keyof Policy) =>
    form.querySelector<HTMLInputElement>(`[data-override="${key}"]`)!;

  function fill(key: keyof Policy, source: Policy) {
    switch (key) {
      case "schedule":
        input("schedule-kind").value = source.schedule.kind;
        input("minutes").value = String(
          source.schedule.kind === "interval" ? source.schedule.minutes : 15,
        );
        input("expression").value =
          source.schedule.kind === "cron"
            ? source.schedule.expression
            : "0 9 * * MON-FRI";
        input("timezone").value = source.schedule.timezone;
        break;
      case "watched_authors":
        input("authors").value = source.watched_authors
          .map(({ id, login }) => `${id}:${login}`)
          .join("\n");
        break;
      case "reviewer_assignment":
        checkbox("reviewer").checked = source.reviewer_assignment;
        break;
      case "adapter":
        input("adapter").value = source.adapter;
        break;
      case "selector":
        input("selector-kind").value = source.selector.kind;
        input("selector-value").value =
          source.selector.kind === "default" ? "" : source.selector.value;
        break;
      case "prompt":
        input("prompt").value = source.prompt;
        break;
      case "automatic_agent_start":
        checkbox("start").checked = source.automatic_agent_start;
        break;
      case "automatic_comment_publication":
        checkbox("publish").checked = source.automatic_comment_publication;
        break;
    }
  }

  function visibility() {
    form.querySelector<HTMLElement>(".interval")!.hidden =
      value("schedule-kind") !== "interval";
    form.querySelector<HTMLElement>(".cron")!.hidden =
      value("schedule-kind") !== "cron";
    form.querySelector<HTMLElement>(".selector-value")!.hidden =
      value("selector-kind") === "default";
  }
  for (const { key } of fields) {
    fill(key, policy);
    if (repository) {
      const toggle = overrideControl(key);
      const fieldset = form.querySelector<HTMLFieldSetElement>(
        `[data-field="${key}"]`,
      )!;
      toggle.checked = overrides[key] != null;
      fieldset.disabled = !toggle.checked;
      toggle.addEventListener("change", () => {
        fieldset.disabled = !toggle.checked;
        if (!toggle.checked) fill(key, defaults);
        visibility();
      });
    }
  }
  visibility();
  input("schedule-kind").addEventListener("change", visibility);
  input("selector-kind").addEventListener("change", visibility);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    try {
      const kind = value("selector-kind");
      if (
        !["default", "model", "agent"].includes(kind) ||
        value("adapter") !== "copilot"
      )
        throw new Error("Unsupported agent configuration.");
      const selected: Selector =
        kind === "default"
          ? { kind }
          : {
              kind: kind === "model" ? "model" : "agent",
              value: value("selector-value").trim(),
            };
      const current: Policy = {
        schedule:
          value("schedule-kind") === "interval"
            ? {
                kind: "interval",
                minutes: Number(value("minutes")),
                timezone: value("timezone").trim(),
              }
            : {
                kind: "cron",
                expression: value("expression").trim(),
                timezone: value("timezone").trim(),
              },
        watched_authors:
          value("authors").trim() === ""
            ? []
            : value("authors")
                .trim()
                .split("\n")
                .map((line) => {
                  const parts = line.trim().split(":");
                  if (parts.length !== 2)
                    throw new Error(
                      "Use one stable GitHub account ID:login per line.",
                    );
                  return { id: parts[0].trim(), login: parts[1].trim() };
                }),
        reviewer_assignment: checkbox("reviewer").checked,
        adapter: "copilot",
        selector: selected,
        prompt: value("prompt"),
        automatic_agent_start: checkbox("start").checked,
        automatic_comment_publication: checkbox("publish").checked,
      };
      if (repository) {
        const selectedOverrides: PolicyOverrides = {
          schedule: overrideControl("schedule").checked
            ? current.schedule
            : undefined,
          watched_authors: overrideControl("watched_authors").checked
            ? current.watched_authors
            : undefined,
          reviewer_assignment: overrideControl("reviewer_assignment").checked
            ? current.reviewer_assignment
            : undefined,
          adapter: overrideControl("adapter").checked
            ? current.adapter
            : undefined,
          selector: overrideControl("selector").checked
            ? current.selector
            : undefined,
          prompt: overrideControl("prompt").checked
            ? current.prompt
            : undefined,
          automatic_agent_start: overrideControl("automatic_agent_start")
            .checked
            ? current.automatic_agent_start
            : undefined,
          automatic_comment_publication: overrideControl(
            "automatic_comment_publication",
          ).checked
            ? current.automatic_comment_publication
            : undefined,
        };
        await invoke("save_repository_policy", {
          id: repository.id,
          overrides: selectedOverrides,
        });
      } else {
        await invoke("save_defaults", { policy: current });
      }
      await reload();
    } catch (cause) {
      showError(
        typeof cause === "string"
          ? cause
          : cause instanceof Error
            ? cause.message
            : "Could not save policy settings.",
      );
      submit.disabled = false;
    }
  });
}
