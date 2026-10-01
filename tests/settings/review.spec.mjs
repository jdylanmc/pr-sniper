import { expect, test } from "./fixtures.mjs";
import {
  section,
  saveChanges,
  repositorySettings,
  closeDialog,
} from "./navigation.mjs";

test("automatic review start is explicit, inherited and saved without enabling publication", async ({
  page,
  store,
}) => {
  await store("save_repository", { repository: "fixture/review-policy" });
  await page.goto("/?view=settings");
  const initial = (await store("snapshot")).settings;
  await section(page, "Preferences");
  const automatic = page.getByRole("switch", {
    name: /^Start eligible reviews automatically/,
  });
  await expect(automatic).not.toBeChecked();
  await automatic.check();
  expect(
    (await store("snapshot")).settings.defaults.automatic_agent_start,
  ).toBe(false);
  await saveChanges(page);
  let settings = (await store("snapshot")).settings;
  expect(settings.defaults.automatic_agent_start).toBe(true);
  expect(settings.defaults.automatic_comment_publication).toBe(
    initial.defaults.automatic_comment_publication,
  );
  await section(page, "Integrations");
  let modal = await repositorySettings(page, "fixture/review-policy");
  await expect(modal.getByLabel("Review start", { exact: true })).toHaveValue(
    "inherit",
  );
  await modal
    .getByLabel("Review start", { exact: true })
    .selectOption("manual");
  await closeDialog(page);
  await saveChanges(page);
  settings = (await store("snapshot")).settings;
  expect(settings.repositories[0].overrides.automatic_agent_start).toBe(false);
  await page.reload();
  modal = await repositorySettings(page, "fixture/review-policy");
  await expect(modal.getByLabel("Review start", { exact: true })).toHaveValue(
    "manual",
  );
  await modal
    .getByLabel("Review start", { exact: true })
    .selectOption("inherit");
  await closeDialog(page);
  await saveChanges(page);
  settings = (await store("snapshot")).settings;
  expect(
    settings.repositories[0].overrides?.automatic_agent_start,
  ).toBeUndefined();
  expect(settings.defaults.automatic_agent_start).toBe(true);
});

function candidate() {
  return {
    key: "exact-revision-key",
    assignment_id: "assignment-1",
    agent_name: "Correctness reviewer",
    job: {
      account_id: "22",
      account_login: "repo-owner",
      repository_name: "example/repo",
      number: 1,
      title: "Review",
      head_sha: "a".repeat(40),
    },
    trust_required: true,
    blocked: null,
    run: null,
  };
}

function run(state) {
  return {
    phase: "Verifying restricted Copilot runtime and reviewing",
    error: null,
    selection: {
      agent: { model: "configured-model", ai_account: { account_id: "33" } },
    },
    operation: {
      id: "operation-1",
      state,
      attempt_count: 1,
      retry_deadline: 1_800_000_900,
    },
    result: null,
  };
}

for (const mode of ["planned", "captured", "legacy", "queued"]) {
  test(`${mode} configuration distinguishes actual execution evidence from today's plan`, async ({
    page,
    store,
  }) => {
    const settings = (await store("snapshot")).settings;
    const selection = {
      agent: {
        id: "agent",
        name: "Captured Agent",
        model: "captured-model",
        ai_account: { provider: "copilot", account_id: "33" },
        prompt: "Captured prompt <script>window.executed=true</script>",
        signature: "machine",
        doctrines: ["Captured doctrine"],
      },
      policy: settings.defaults,
      doctrine: "Captured doctrine body.",
      preset: null,
      configuration: {
        repository: {
          id: "repo",
          provider: "github",
          name: "example/repo",
          enabled: true,
          provider_account_id: "22",
          provider_repository_id: "100",
          assignments: [],
        },
        authority: {
          primary: true,
          comment: false,
          approve: false,
          merge: false,
        },
        doctrines: [
          { title: "Captured doctrine", body: "Captured doctrine body." },
        ],
      },
    };
    const planned = structuredClone(selection);
    planned.agent.prompt = "Today's planned prompt.";
    planned.configuration.doctrines[0].body = "Today's doctrine body.";
    const review = {
      ...candidate(),
      planned_selection: planned,
      run:
        mode === "planned"
          ? null
          : {
              ...run(mode === "queued" ? "queued" : "completed"),
              selection,
              job: candidate().job,
            },
    };
    if (mode === "legacy") delete review.run.selection.configuration;
    await page.addInitScript((review) => {
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) =>
        command === "monitoring_snapshot"
          ? Promise.resolve({ health: [], jobs: [], reviews: [review] })
          : original(command, args);
    }, review);
    await page.goto("/?view=queue");
    const panel = page.locator("#agent-reviews");
    if (mode !== "planned") {
      await panel
        .getByText("Captured execution configuration", { exact: true })
        .click();
      await expect(panel).toContainText(selection.agent.prompt);
      expect(await page.evaluate(() => window.executed)).toBeUndefined();
      await expect(panel.locator("script")).toHaveCount(0);
    }
    if (mode === "planned" || mode === "queued") {
      await panel
        .getByText(
          "Planned configuration (revalidated at start; not execution evidence)",
          { exact: true },
        )
        .click();
      await expect(panel).toContainText("Today's planned prompt.");
      await expect(panel).toContainText("Today's doctrine body.");
    } else {
      await expect(panel).not.toContainText("Today's planned prompt.");
      await expect(panel).not.toContainText("Today's doctrine body.");
    }
    if (mode === "legacy")
      await expect(panel).toContainText(
        "Today's settings are not historical evidence",
      );
    if (mode === "captured")
      await expect(panel).toContainText(
        "Captured assignment authority (not a current provider grant)",
      );
  });
}

test("review start is explicit, revision-bound and requires trust consent", async ({
  page,
}) => {
  await page.addInitScript((review) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__reviewActions = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "monitoring_snapshot")
        return Promise.resolve({ health: [], jobs: [], reviews: [review] });
      if (command === "start_review") {
        window.__reviewActions.push({ command, args });
        return Promise.resolve();
      }
      return original(command, args);
    };
  }, candidate());
  await page.goto("/?view=queue");
  const start = page.getByRole("button", { name: "Start review", exact: true });
  await expect(start).toBeDisabled();
  const trust = page.getByRole("checkbox");
  await trust.check();
  await expect(start).toBeEnabled();
  await start.click();
  await expect
    .poll(() => page.evaluate(() => window.__reviewActions))
    .toEqual([
      {
        command: "start_review",
        args: { candidateKey: "exact-revision-key", confirmTrust: true },
      },
    ]);
  await expect(start).toBeDisabled();
});

test("running review exposes cancellation and safe visible failures", async ({
  page,
}) => {
  const review = { ...candidate(), trust_required: false, run: run("running") };
  await page.addInitScript((review) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__reviewActions = [];
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "monitoring_snapshot")
        return Promise.resolve({ health: [], jobs: [], reviews: [review] });
      if (command === "cancel_review") {
        window.__reviewActions.push(args);
        review.run.operation.state = "failed";
        review.run.phase = "Cancelled";
        review.run.error = "Review cancelled by user.";
        return Promise.resolve();
      }
      return original(command, args);
    };
  }, review);
  await page.goto("/?view=queue");
  await expect(page.locator("#agent-reviews")).toContainText(
    "Copilot account: 33; model: configured-model",
  );
  await page
    .getByRole("button", { name: "Cancel review", exact: true })
    .click();
  await expect(
    page.getByText("Review cancelled by user.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Retry review", exact: true }),
  ).toBeEnabled();
  expect(await page.evaluate(() => window.__reviewActions)).toEqual([
    { operationId: "operation-1" },
  ]);
});

test("completed review renders every file safely without inventing publication state or approval", async ({
  page,
}) => {
  const review = {
    ...candidate(),
    trust_required: false,
    run: run("completed"),
  };
  review.run.phase = "Automated review complete; final human review required";
  review.run.result = {
    output: {
      synopsis: "The change needs human review.",
      decision: "human_input_required",
      files: Array.from({ length: 301 }, (_, i) => ({
        path: i === 300 ? "<img src=x onerror=alert(1)>.rs" : `file-${i}.rs`,
        order: i + 1,
        explanation: "Read this changed file.",
      })),
      findings: [
        {
          path: "file-0.rs",
          side: "head",
          line: 4,
          severity: "high",
          title: "Incorrect branch",
          explanation: "The condition skips the new path.",
          confidence: 90,
        },
      ],
    },
    session_id: "session-1",
    model: "configured-model",
    runtime_version: "synthetic",
    input_tokens: 100,
    output_tokens: 40,
    tool_calls: 20,
  };
  await page.addInitScript((review) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "monitoring_snapshot"
        ? Promise.resolve({ health: [], jobs: [], reviews: [review] })
        : original(command, args);
  }, review);
  await page.goto("/?view=queue");
  const root = page.locator("#agent-reviews");
  await expect(root).toContainText("Human input required");
  await root
    .getByText("Complete file guide (301 files)", { exact: true })
    .click();
  await expect(root.locator("ol li")).toHaveCount(301);
  await expect(root.locator("ol li").last()).toHaveText(
    "<img src=x onerror=alert(1)>.rs: Read this changed file.",
  );
  await expect(root.locator("img")).toHaveCount(0);
  await expect(root.getByRole("button")).toHaveCount(0);
  await expect(root).toContainText("Session session-1");
});
