import { expect, test } from "./fixtures.mjs";

const waitingStates = [
  [
    "trust_confirmation",
    "Waiting for explicit trust confirmation; no review has started.",
  ],
  [
    "human_start",
    "Automatic start is disabled; manual review start is not implemented here.",
  ],
  [
    "agent_unavailable",
    "Automatic start is configured, but review-agent support is not implemented here.",
  ],
  [
    "account_disconnected",
    "Not actionable: the acting GitHub account is disconnected.",
  ],
  ["repository_disabled", "Not actionable: repository monitoring is disabled."],
  [
    "repository_removed",
    "Not actionable: the repository was removed from Settings.",
  ],
  [
    "binding_changed",
    "Not actionable: the repository account binding changed.",
  ],
  ["policy_changed", "Not actionable: the trigger policy changed."],
  [
    "superseded",
    "Not actionable: a newer head revision superseded this detection.",
  ],
  ["ineligible", "Not actionable: this revision is no longer eligible."],
  [
    "no_longer_current",
    "Not seen in the latest open-PR scan; rechecked next scan. This is not a terminal closed or merged state.",
  ],
];

test("Review Queue renders acting schedule identity and every persisted detection state", async ({
  page,
}) => {
  const snapshot = {
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
      watched_author: true,
      requested_reviewer: false,
      waiting,
    })),
  };
  await page.addInitScript((value) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__monitoringChecks = 0;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "monitoring_snapshot") return Promise.resolve(value);
      if (command === "check_now") {
        window.__monitoringChecks++;
        return Promise.resolve();
      }
      return invoke(command, args);
    };
  }, snapshot);

  await page.goto("/?view=queue");
  const health = page.locator("#schedule-health");
  await expect(
    health.locator("p").filter({ hasText: "scheduled/repo" }),
  ).toContainText("scheduled/repo: Scheduled; immediate check pending.");
  await expect(
    health.locator("p").filter({ hasText: "checking/repo" }),
  ).toContainText("checking/repo: Checking.");
  const blocked = health.locator("p").filter({ hasText: "unbound/repo" });
  await expect(blocked).toContainText("unbound/repo: Blocked.");
  await expect(blocked).toContainText("Acting account: unbound");
  await expect(blocked).toContainText("Next run: Unavailable");
  await expect(
    health.locator("p").filter({ hasText: "unavailable/repo" }),
  ).toContainText("unavailable/repo: Unavailable.");
  await expect(
    health.locator("p").filter({ hasText: "disabled/repo" }),
  ).toContainText("disabled/repo: Disabled.");
  await expect(health).toContainText("current-login (22)");
  await expect(health).toContainText(
    "Security reviewer (agent-1), assignment assignment-1",
  );
  await expect(health).toContainText("interval:5:America/New_York");
  await expect(health).toContainText("stable ID 23 (login unavailable)");

  for (const [, label] of waitingStates)
    await expect(page.locator("#review-jobs")).toContainText(label);
  await expect(
    page.locator("#review-jobs article").filter({
      hasText:
        "Automatic start is configured, but review-agent support is not implemented here.",
    }),
  ).toHaveCount(1);

  await page.getByRole("button", { name: "Check Now", exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => window.__monitoringChecks))
    .toBe(1);
});
