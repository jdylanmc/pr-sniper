import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

async function park(store, failure, second) {
  const fixture = await queueFixture(store);
  const publication = fixture.state.publications.find(
    (entry) => entry.review.job.number === 3,
  );
  publication.operation.failure = failure;
  if (second) {
    const other = fixture.state.publications.find(
      (entry) => entry.review.job.number === 2,
    );
    other.operation.state = "manual_retry";
    other.operation.failure = second;
  }
  await store("seed_queue_state", fixture.state);
}

test("a publication's credentials park routes Fix to its GitHub account and offers Retry", async ({
  page,
  store,
}) => {
  await park(store, "credentials");
  await page.goto("/");
  await showQueueGroup(page, "attention");
  const failed = page.getByRole("article", {
    name: "example/repo #3",
    exact: true,
  });
  await expect(failed).toHaveAttribute("data-state", "failed");
  await expect(failed).toContainText("Credentials");
  await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
  await expect(failed.locator("[data-fix-account]")).toHaveText("Fix");
  await expect(page.locator("[data-queue-attention]")).toHaveText("2");
  await failed.locator("[data-fix-account]").click();
  await expect(
    page.getByRole("heading", { name: "GitHub", exact: true }),
  ).toBeVisible();
});

for (const [origin, heading] of [
  ["copilot", "GitHub Copilot"],
  ["github", "GitHub"],
  [undefined, "Accounts"],
]) {
  test(`a review's ${origin ?? "legacy"} credentials park routes Fix to ${heading}`, async ({
    page,
    store,
  }) => {
    const fixture = await queueFixture(store);
    const review = fixture.state.reviews.find(
      (entry) => entry.job.number === 3,
    );
    review.operation.state = "failed";
    review.operation.failure = "credentials";
    if (origin) review.operation.credentials_origin = origin;
    review.result = null;
    fixture.state.publications = fixture.state.publications.filter(
      (entry) => entry.review.job.number !== 3,
    );
    await store("seed_queue_state", fixture.state);
    await page.goto("/");
    await showQueueGroup(page, "attention");
    const failed = page.getByRole("article", {
      name: "example/repo #3",
      exact: true,
    });
    await expect(failed).toContainText("Credentials");
    await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
    await failed.locator("[data-fix-account]").click();
    await expect(
      page.getByRole("heading", { name: heading, exact: true }),
    ).toBeVisible();
  });
}

// A teammate's model refused service while its account stayed connected.
async function parkModel(page, store, extra = () => {}) {
  const fixture = await queueFixture(store);
  const review = fixture.state.reviews.find((entry) => entry.job.number === 3);
  review.operation.state = "manual_retry";
  review.operation.failure = "model";
  review.result = null;
  review.failed_model = {
    agent_id: review.selection.agent.id,
    agent_name: review.selection.agent.name,
    model: "example-model-fast",
    refusal: "rejected",
    status: 401,
  };
  fixture.state.publications = fixture.state.publications.filter(
    (entry) => entry.review.job.number !== 3,
  );
  extra(fixture.state, review);
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "copilot_auth_state")
        return Promise.resolve({
          accounts: [
            {
              provider: "copilot",
              account_id: "33",
              login: "fixture-ai",
              state: "connected",
            },
          ],
          flow: { state: "idle" },
        });
      if (command === "list_copilot_models")
        return Promise.resolve([
          { id: "example-model-fast", name: "Example fast" },
          { id: "example-model", name: "Example" },
        ]);
      return invoke(command, args);
    };
  });
  return (await store("monitoring_snapshot")).items.find(
    (entry) => entry.job.number === 3,
  );
}

async function expectModelPicker(page) {
  const editor = page.getByRole("dialog", {
    name: "Editing Correctness reviewer",
  });
  await expect(editor).toBeVisible();
  await expect(editor.getByLabel("Model", { exact: true })).toBeFocused();
}

test("a model refusal names the Agent and model, and Fix lands on that Agent's model picker", async ({
  page,
  store,
}) => {
  const item = await parkModel(page, store);
  expect(item.failure_class).toBe("model");
  expect(item.failed_model.model).toBe("example-model-fast");
  await page.goto("/");
  await showQueueGroup(page, "attention");
  const failed = page.getByRole("article", {
    name: "example/repo #3",
    exact: true,
  });
  await expect(failed.locator(".queue-state")).toHaveText("Model");
  const chip = failed.locator("[data-queue-model]");
  await expect(chip).toHaveAttribute("data-queue-model", "example-model-fast");
  await expect(chip).toHaveAttribute("data-model-refusal", "rejected");
  await expect(chip).toHaveText("example-model-fast401");
  await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
  await failed.getByRole("button", { name: "Fix: change model" }).click();
  await expectModelPicker(page);
});

test("a model refusal's detail shows Agent, model and status with Change model first", async ({
  page,
  store,
}) => {
  const item = await parkModel(page, store);
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  const status = page.locator("[data-queue-status]");
  await expect(status.locator("[data-failure-cause]")).toHaveAttribute(
    "data-failure-cause",
    "model_rejected",
  );
  await expect(status.locator(".queue-status-label")).toHaveText(
    "Model rejected",
  );
  const facts = status.locator(".queue-facts");
  for (const [term, value] of [
    ["Cause", "Model / rejected"],
    ["Agent", "Correctness reviewer"],
    ["Model", "example-model-fast"],
    ["HTTP", "401"],
  ])
    await expect(
      facts.locator("dt", { hasText: term }).locator("+ dd"),
    ).toHaveText(value);
  const primary = status.locator("[data-queue-primary]");
  await expect(primary).toHaveAttribute("data-queue-primary", "change-model");
  await expect(primary).toHaveText("Change model");
  await expect(status.getByRole("button", { name: "Retry" })).toHaveCount(1);
  await primary.click();
  await expectModelPicker(page);
});

test("a repository check still retrying never hides a parked model refusal in the detail", async ({
  page,
  store,
}) => {
  const now = Math.floor(Date.now() / 1000);
  const item = await parkModel(page, store, (state, review) => {
    state.monitoring = {
      health: {
        [review.job.configuration_id]: {
          repository_id: review.job.configuration_id,
          name: "example/repo",
          schedule_key: "fixture",
          provider_account_id: "22",
          account_login: "local-operator",
          provider_repository_id: "100",
          enabled: true,
          last_attempt: now - 60,
          last_success: null,
          next_run: 0,
          schedule_available: true,
          last_failure: "network",
          last_error: "Network error.",
          in_flight: false,
          operation: {
            id: "health-op",
            provider: "github",
            account_id: "22",
            configuration_id: review.job.configuration_id,
            repository_id: "100",
            pull_request_id: null,
            head_sha: null,
            trigger_policy: "fixture",
            operation_type: "repository_poll",
            state: "queued",
            attempt_count: 1,
            initial_attempt_at: now - 60,
            retry_deadline: now + 600,
            next_attempt_at: now + 60,
            failure: "network",
            attempted_mutation: null,
            pending_review_id: null,
            owned_thread_id: null,
            triggering_external_comment_id: null,
            confirmed_receipt: null,
          },
        },
      },
    };
  });
  expect(item.failure_class).toBe("model");
  await store("panel_navigate", {
    route: { tab: "queue", detail: { type: "item", item_id: item.id } },
  });
  await page.goto("/");
  const status = page.locator("[data-queue-status]");
  await expect(status.locator("[data-failure-cause]")).toHaveAttribute(
    "data-failure-cause",
    "model_rejected",
  );
  const primary = status.locator("[data-queue-primary]");
  await expect(primary).toHaveAttribute("data-queue-primary", "change-model");
  await primary.click();
  await expectModelPicker(page);
});

test("every parked failure is one Needs attention row with a cause chip, Retry and never a Running row", async ({
  page,
  store,
}) => {
  for (const failure of ["network", "permission", "timeout", "permanent"]) {
    await park(store, failure);
    await store("panel_navigate", { route: { tab: "queue" } });
    await page.goto("/");
    await showQueueGroup(page, "attention");
    const failed = page.getByRole("article", {
      name: "example/repo #3",
      exact: true,
    });
    await expect(failed).toHaveCount(1);
    await expect(failed).toHaveAttribute("data-state", "failed");
    await expect(failed.locator(".queue-state")).toHaveText(
      {
        network: "Network",
        permission: "Permission",
        timeout: "Network",
        permanent: "Unverified",
      }[failure],
    );
    await expect(failed.locator("[data-fix-account]")).toHaveCount(0);
    await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
    await expect(page.locator("[data-queue-attention]")).toHaveText("2");
    await tab(page, "Running").click();
    await expect(page.locator('[data-work-group="failures"]')).toHaveCount(0);
    await expect(page.locator("[data-running-list] article")).toHaveCount(0);
    await expect(page.locator(".work-retry-all")).toHaveCount(0);
  }
});

test("Retry all heads Needs attention and counts every parked row, credentials included", async ({
  page,
  store,
}) => {
  await park(store, "network", "permission");
  await page.goto("/");
  await showQueueGroup(page, "attention");
  const all = page.getByRole("button", { name: "Retry all failed" });
  await expect(all).toHaveText("2");
  await expect(
    page
      .locator('[data-queue-group="attention"]')
      .first()
      .locator("> *")
      .first(),
  ).toHaveClass(/work-retry-all/);
  await park(store, "credentials", "permission");
  await page.goto("/");
  await showQueueGroup(page, "attention");
  await expect(all).toHaveText("2");
  await expect(
    page.getByRole("article", { name: "example/repo #3", exact: true }),
  ).toContainText("Credentials");
});
