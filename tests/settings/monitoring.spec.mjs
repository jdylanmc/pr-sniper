import { expect, test } from "./fixtures.mjs";

const waitingStates = [
  ["trust_confirmation", "Waiting"],
  ["human_start", "Queued"],
  ["agent_unavailable", "Agent unavailable"],
  ["account_disconnected", "Account disconnected"],
  ["repository_disabled", "Repository disabled"],
  ["repository_removed", "Repository removed"],
  ["binding_changed", "Binding changed"],
  ["policy_changed", "Policy changed"],
  ["superseded", "Superseded"],
  ["ineligible", "Ineligible"],
  ["no_longer_current", "Not current"],
  ["scope_excluded", "Out of scope"],
  ["closed", "Closed"],
  ["merged", "Merged"],
  ["assignment_removed", "Assignment removed"],
];

test("Review Queue renders acting schedule identity and every persisted detection state", async ({
  page,
}) => {
  const snapshot = {
    global_scan: {
      schedule_key: "cron:*/5 * * * *:America/New_York",
      next_run: 1_800_000_300,
      pending: [],
    },
    health: [
      {
        repository_id: "scheduled",
        name: "scheduled/repo",
        schedule_key: "interval:5:America/New_York",
        provider_account_id: "22",
        account_login: "current-login",
        assignment_id: "assignment-1",
        agent_id: "agent-1",
        agent_name: "Security reviewer",
        enabled: true,
        last_attempt: 1_800_000_000,
        last_success: 1_800_000_001,
        next_run: 1_800_000_300,
        schedule_available: true,
        last_failure: null,
        in_flight: false,
        manual_pending: true,
      },
      {
        repository_id: "checking",
        name: "checking/repo",
        schedule_key: "interval:10:UTC",
        provider_account_id: "22",
        account_login: "current-login",
        assignment_id: null,
        agent_id: null,
        agent_name: null,
        enabled: true,
        last_attempt: 1_800_000_010,
        last_success: null,
        next_run: 1_800_000_600,
        schedule_available: true,
        last_failure: null,
        in_flight: true,
        manual_pending: false,
      },
      {
        repository_id: "blocked",
        name: "unbound/repo",
        schedule_key: "cron:0 9 * * MON-FRI:UTC",
        provider_account_id: null,
        account_login: null,
        assignment_id: null,
        agent_id: null,
        agent_name: null,
        enabled: true,
        last_attempt: null,
        last_success: null,
        next_run: 0,
        schedule_available: false,
        last_failure: "account_binding_required",
        in_flight: false,
        manual_pending: false,
      },
      {
        repository_id: "unavailable",
        name: "unavailable/repo",
        schedule_key: "",
        provider_account_id: "23",
        account_login: null,
        assignment_id: null,
        agent_id: null,
        agent_name: null,
        enabled: true,
        last_attempt: null,
        last_success: null,
        next_run: 0,
        schedule_available: false,
        last_failure: null,
        in_flight: false,
        manual_pending: false,
      },
      {
        repository_id: "scope-required",
        name: "scope-required/repo",
        schedule_key: "",
        provider_account_id: "25",
        account_login: "scope-owner",
        assignment_id: null,
        agent_id: null,
        agent_name: null,
        enabled: true,
        last_attempt: null,
        last_success: null,
        next_run: 0,
        schedule_available: false,
        last_failure: "scope_confirmation_required",
        in_flight: false,
        manual_pending: false,
      },
      {
        repository_id: "manual-retry",
        name: "retry/repo",
        schedule_key: "interval:15:UTC",
        provider_account_id: "22",
        account_login: "current-login",
        assignment_id: null,
        agent_id: null,
        agent_name: null,
        enabled: true,
        last_attempt: 1_800_000_020,
        last_success: null,
        next_run: 0,
        schedule_available: false,
        last_failure: "MissingScope",
        in_flight: false,
        manual_pending: false,
        operation: {
          id: "operation-1",
          operation_type: "repository_poll",
          state: "manual_retry",
          attempt_count: 1,
          retry_deadline: 1_800_000_900,
          next_attempt_at: null,
          failure: "permanent",
        },
      },
      {
        repository_id: "disabled",
        name: "disabled/repo",
        schedule_key: "interval:15:UTC",
        provider_account_id: "24",
        account_login: "disabled-account",
        assignment_id: null,
        agent_id: null,
        agent_name: null,
        enabled: false,
        last_attempt: null,
        last_success: null,
        next_run: 0,
        schedule_available: false,
        last_failure: null,
        in_flight: false,
        manual_pending: false,
      },
    ],
    jobs: waitingStates.map(([waiting], index) => ({
      account_id: "22",
      account_login: "current-login",
      repository_name: "example/repo",
      number: index + 1,
      title: `Detection ${waiting}`,
      head_sha: String(index).padStart(40, "a"),
      author_id: "11",
      author_login: "author",
      watched_author: index !== 1,
      all_authors: index === 1,
      requested_reviewer: false,
      waiting,
    })),
  };
  await page.addInitScript((value) => {
    const wrap = (internals) => {
      const invoke = internals.invoke;
      internals.invoke = (command, args) => {
        if (command === "monitoring_snapshot")
          return Promise.resolve(window.__monitoringSnapshot);
        if (command === "check_now") {
          window.__monitoringChecks++;
          return Promise.resolve();
        }
        if (command === "retry_monitoring_operation") {
          window.__monitoringRetries.push(args.operationId);
          return Promise.resolve();
        }
        return invoke(command, args);
      };
      return internals;
    };
    window.__monitoringChecks = 0;
    window.__monitoringRetries = [];
    window.__monitoringSnapshot = value;
    if (window.__TAURI_INTERNALS__) {
      window.__TAURI_INTERNALS__ = wrap(window.__TAURI_INTERNALS__);
      return;
    }
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      set(internals) {
        Object.defineProperty(window, "__TAURI_INTERNALS__", {
          configurable: true,
          value: wrap(internals),
          writable: true,
        });
      },
    });
  }, snapshot);

  await page.goto("/?view=queue");
  const health = page.locator("#schedule-health");
  await expect(
    health.locator("p").filter({ hasText: "scheduled/repo" }),
  ).toContainText("scheduled/repo: Scheduled; check pending");
  await expect(
    health.locator("p").filter({ hasText: "checking/repo" }),
  ).toContainText("checking/repo: Checking;");
  const blocked = health.locator("p").filter({ hasText: "unbound/repo" });
  await expect(blocked).toContainText("unbound/repo: Failed;");
  await expect(blocked).toContainText("account Unbound;");
  await expect(blocked).toContainText("next Unavailable;");
  await expect(
    health.locator("p").filter({ hasText: "unavailable/repo" }),
  ).toContainText("unavailable/repo: Unavailable;");
  await expect(
    health.locator("p").filter({ hasText: "disabled/repo" }),
  ).toContainText("disabled/repo: Disabled;");
  await expect(
    health.locator("p").filter({ hasText: "scope-required/repo" }),
  ).toContainText("scope-required/repo: Failed;");
  const retry = health.locator("p").filter({ hasText: "retry/repo" });
  await expect(retry).toContainText("retry/repo: Manual retry;");
  await retry.getByRole("button", { name: "Retry", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => window.__monitoringRetries))
    .toEqual(["operation-1"]);
  await expect(health).toContainText("current-login (22)");
  await expect(health).toContainText(
    "Security reviewer (agent-1), assignment assignment-1",
  );
  await expect(health).toContainText("interval:5:America/New_York");
  await expect(health).toContainText("account 23;");

  for (const [, label] of waitingStates)
    await expect(page.locator("#review-jobs")).toContainText(label);
  await expect(
    page.locator("#review-jobs article").filter({
      hasText: "Agent unavailable",
    }),
  ).toHaveCount(1);
  await expect(page.locator("#review-jobs")).toContainText(
    "all-author monitoring scope",
  );

  await page.getByRole("button", { name: "Check Now", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => window.__monitoringChecks))
    .toBe(1);
  await page.evaluate(() => {
    for (const repository of window.__monitoringSnapshot.health) {
      repository.enabled = false;
      repository.schedule_available = false;
      repository.next_run = 0;
    }
    window.__monitoringSnapshot.global_scan.pending = [];
    window.__monitoringSnapshot.global_scan.next_run = 0;
    const invalidCron = window.__monitoringSnapshot.health.find(
      (repository) => repository.repository_id === "disabled",
    );
    invalidCron.enabled = true;
    invalidCron.last_failure = "invalid_global_cron";
  });
  await page.getByRole("button", { name: "Check Now", exact: true }).click();
  await expect(
    health.locator("p").filter({ hasText: "Global scan:" }),
  ).toContainText("next Unavailable");
  await page.evaluate(() => {
    const invalidCron = window.__monitoringSnapshot.health.find(
      (repository) => repository.repository_id === "disabled",
    );
    invalidCron.enabled = false;
  });
  await page.getByRole("button", { name: "Check Now", exact: true }).click();
  await expect(
    health.locator("p").filter({ hasText: "Global scan:" }),
  ).toContainText("next None scheduled");
  await expect
    .poll(() => page.evaluate(() => window.__monitoringChecks))
    .toBe(3);
});
