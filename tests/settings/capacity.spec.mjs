import { test, expect } from "./fixtures.mjs";
import { section, seedAgent, editAgent } from "./navigation.mjs";
import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { queueFixture } from "./queue-fixture.mjs";

test("pause persists across queue and Settings without saving unrelated preferences or granting actions", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  const original = (await store("snapshot")).settings;
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("20");
  await page
    .getByRole("button", { name: "Pause automation", exact: true })
    .click();
  await expect(page.locator("[data-automation-status]")).toContainText(
    "Paused; 0 occupied / 4",
  );
  expect((await store("snapshot")).settings).toEqual(original);
  expect((await store("automation_snapshot")).paused).toBe(true);
  await expect(page.locator("#global-capacity")).toHaveValue("20");
  await page
    .getByRole("button", { name: "Save preferences", exact: true })
    .click();
  await expect(page.locator("#save-status")).toHaveText("All changes saved");
  await expect(page.locator("[data-automation-status]")).toContainText(
    "Paused; 0 occupied / 20",
  );
  await page.goto("/?view=queue");
  await expect(page.locator("[data-automation-status]")).toContainText(
    "Paused; 0 occupied / 20",
  );
  await expect(page.locator("#automation-controls")).toContainText(
    "Already-started remote mutations may have succeeded",
  );
  await page
    .getByRole("button", { name: "Resume automation", exact: true })
    .click();
  await expect(page.locator("[data-automation-status]")).toContainText(
    "Running; 0 occupied / 20",
  );
  expect(
    (await store("snapshot")).settings.defaults.automatic_agent_start,
  ).toBe(false);
  expect(
    (await store("snapshot")).settings.defaults.automatic_comment_publication,
  ).toBe(false);
});

test("capacity accepts one and twenty, rejects zero and retains independent Agent saves", async ({
  page,
  store,
}) => {
  await seedAgent(store);
  await page.goto("/?view=settings");
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("20");
  const agent = await editAgent(page);
  await agent
    .getByRole("textbox", { name: "Prompt", exact: true })
    .fill("Independent prompt.");
  await agent.getByRole("button", { name: "Save agent", exact: true }).click();
  await expect(agent).toHaveCount(0);
  expect((await store("snapshot")).settings.capacity).toBe(4);
  await section(page, "Preferences");
  await expect(page.locator("#global-capacity")).toHaveValue("20");
  for (const value of ["20", "1"]) {
    await page.locator("#global-capacity").fill(value);
    await page
      .getByRole("button", { name: "Save preferences", exact: true })
      .click();
    await expect(page.locator("#save-status")).toHaveText("All changes saved");
    expect((await store("snapshot")).settings.capacity).toBe(Number(value));
    expect((await store("snapshot")).settings.agents[0].prompt).toBe(
      "Independent prompt.",
    );
  }
  const before = (await store("snapshot")).settings;
  await page.locator("#global-capacity").fill("0");
  await page
    .getByRole("button", { name: "Save preferences", exact: true })
    .click();
  await expect(page.locator("#error")).toContainText("positive whole number");
  await expect(page.locator("#global-capacity")).toHaveValue("0");
  expect((await store("snapshot")).settings).toEqual(before);
});

test("failed pause write is visible and never claims paused state", async ({
  page,
  store,
  dataRoot,
}) => {
  await page.goto("/?view=queue");
  await expect(page.locator("[data-automation-status]")).toContainText(
    "Running;",
  );
  await mkdir(join(dataRoot, "state/automation.json.tmp"), { recursive: true });
  await page
    .getByRole("button", { name: "Pause automation", exact: true })
    .click();
  await expect(page.locator("[data-automation-error]")).toBeVisible();
  expect((await store("automation_snapshot")).paused).toBe(false);
  await expect(
    page.getByRole("button", { name: "Pause automation", exact: true }),
  ).toBeEnabled();
  await expect(
    readFile(join(dataRoot, "state/automation.json")),
  ).rejects.toMatchObject({ code: "ENOENT" });
});

test("native occupancy and stopping counts stay explicit and FIFO text is inert", async ({
  page,
}) => {
  const state = {
    paused: false,
    capacity: 4,
    active: 4,
    stopping: 0,
    waiting: 3,
    blocked: 1,
    work: Array.from({ length: 8 }, (_, index) => ({
      key: { kind: index === 1 ? "reply" : "normal", id: `work-${index + 1}` },
      enqueue_order: index + 1,
      state: index < 4 ? "active" : index === 7 ? "blocked" : "waiting",
      reason:
        index === 7
          ? "<script>window.executed=true</script> Trust required."
          : null,
    })),
  };
  await page.addInitScript((initial) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    const snapshot = initial;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "automation_snapshot") return structuredClone(snapshot);
      if (command === "set_automation_paused") {
        snapshot.paused = args.paused;
        snapshot.stopping = 4;
        snapshot.work.slice(0, 4).forEach((work) => (work.state = "stopping"));
        return;
      }
      return original(command, args);
    };
  }, state);
  await page.goto("/?view=queue");
  await expect(page.locator("[data-automation-status]")).toHaveText(
    "Running; 4 occupied / 4 AI slots (0 stopping); 3 waiting; 1 blocked.",
  );
  await expect(page.locator("[data-ai-work] li")).toHaveCount(8);
  await expect(page.locator("[data-ai-work] li").nth(1)).toContainText(
    "reply work-2: active; queue order 2",
  );
  await expect(page.locator("[data-ai-work] script")).toHaveCount(0);
  expect(await page.evaluate(() => window.executed)).toBeUndefined();
  await page
    .getByRole("button", { name: "Pause automation", exact: true })
    .click();
  await expect(page.locator("[data-automation-status]")).toHaveText(
    "Paused; 4 occupied / 4 AI slots (4 stopping); 3 waiting; 1 blocked.",
  );
  await page
    .getByRole("button", { name: "Resume automation", exact: true })
    .click();
  await expect(page.locator("[data-automation-status]")).toHaveText(
    "Running; 4 occupied / 4 AI slots (4 stopping); 3 waiting; 1 blocked.",
  );
});

test("a running review no longer prevents accepting another manual start request", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  fixture.state.publications = [];
  fixture.state.reviews = fixture.state.reviews.slice(0, 1);
  fixture.state.reviews[0].operation.state = "running";
  fixture.state.reviews[0].result = null;
  await store("seed_queue_state", fixture.state);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__starts = [];
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (command === "start_review") {
        window.__starts.push(args);
        return;
      }
      return original(command, args);
    };
  });
  await page.goto("/?view=queue");
  const start = page
    .locator("#agent-reviews article")
    .filter({ hasText: "example/repo #9" })
    .getByRole("button", { name: "Start review", exact: true });
  await expect(start).toBeEnabled();
  await start.click();
  await expect.poll(() => page.evaluate(() => window.__starts.length)).toBe(1);
});
