import { test, expect } from "./fixtures.mjs";
import {
  providerFixture,
  repositoryPage,
  reviewer,
} from "./repository-provider-fixture.mjs";
import { repositorySettings, section } from "./navigation.mjs";
import { mkdir, rmdir } from "node:fs/promises";
import { join } from "node:path";

async function configured(store, enabled = false) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  settings.repositories = [
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      provider: "github",
      name: "fixture/one",
      enabled,
      provider_account_id: "22",
      provider_repository_id: "100",
      assignments: [
        {
          id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
          agent_id: reviewer.id,
          schedule: settings.defaults.schedule,
          comment: false,
          actions: { reply: false, approve: false, merge: false },
        },
      ],
    },
  ];
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

for (const failure of ["write", "conflict"]) {
  test(`repository switch ${failure} cannot report authorization and retains draft for repair`, async ({
    page,
    store,
    dataRoot,
  }) => {
    const initial = await configured(store);
    await providerFixture(page, store);
    await repositoryPage(page, store);
    const editor = await repositorySettings(page, "fixture/one");
    await editor
      .getByLabel("Reviewer requests", { exact: true })
      .selectOption("on");
    if (failure === "write")
      await mkdir(join(dataRoot, "config/settings.json.tmp"));
    else {
      const next = structuredClone(initial);
      next.repositories[0].overrides = { reviewer_assignment: false };
      await store("seed_settings", next);
    }
    await editor.getByRole("switch", { name: "Monitor fixture/one" }).click();
    await expect(editor.locator("[data-resource-error]")).toBeVisible();
    await expect(
      editor.getByRole("switch", { name: "Monitor fixture/one" }),
    ).not.toBeChecked();
    await expect(
      editor.getByLabel("Reviewer requests", { exact: true }),
    ).toHaveValue("on");
    expect(
      (await store("snapshot")).settings.repository_authorizations,
    ).toBeUndefined();
    expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
      false,
    );
    if (failure === "write") {
      await rmdir(join(dataRoot, "config/settings.json.tmp"));
      await editor.getByRole("switch", { name: "Monitor fixture/one" }).click();
      await expect(
        editor.locator("[data-repository-monitoring-state]"),
      ).toHaveText("Monitoring: Enabled");
      await editor
        .getByRole("button", { name: "Save repository", exact: true })
        .click();
      await expect(editor).toHaveCount(0);
      expect(
        Object.keys(
          (await store("snapshot")).settings.repository_authorizations,
        ),
      ).toHaveLength(1);
    }
  });
}

test("an explicit disabled Save and unrelated Preferences never enable a paused repository", async ({
  page,
  store,
}) => {
  const initial = await configured(store);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  let editor = await repositorySettings(page, "fixture/one");
  await expect(
    editor.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  await editor
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(editor).toHaveCount(0);
  await section(page, "Preferences");
  await page.getByLabel("AI capacity", { exact: true }).fill("7");
  await page
    .getByRole("button", { name: "Save preferences", exact: true })
    .click();
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories).toEqual(initial.repositories);
  expect(Object.values(saved.repository_authorizations ?? {})).toEqual([null]);
  editor = await repositorySettings(page, "fixture/one");
  await expect(
    editor.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
});

test("Save locks dismissal until the native commit returns and does not issue a scope or provider read", async ({
  page,
  store,
  ipc,
}) => {
  await configured(store);
  const fixture = await providerFixture(page, store);
  await repositoryPage(page, store);
  const editor = await repositorySettings(page, "fixture/one");
  await editor.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(editor.locator("[data-repository-monitoring-state]")).toHaveText(
    "Monitoring: Enabled",
  );
  const held = ipc.holdNext("save_resource");
  try {
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await held.arrived;
    await expect(
      editor.getByRole("button", { name: "Close dialog", exact: true }),
    ).toBeDisabled();
    await expect(
      editor.getByRole("button", { name: "Cancel repository changes" }),
    ).toBeDisabled();
    expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
      true,
    );
    held.release();
    await expect(editor).toHaveCount(0);
    expect(
      fixture.calls.filter((c) =>
        /preview|activation|pull_requests|resolve_provider_repository/.test(
          c.command,
        ),
      ),
    ).toEqual([]);
  } finally {
    held.release();
  }
});

for (const embedded of [true, false]) {
  test(`Genie uses repository Save as authorization and finishes without a second consent (${embedded})`, async ({
    page,
    store,
  }, info) => {
    await configured(store);
    await providerFixture(page, store);
    await repositoryPage(page, store, embedded);
    let editor = await repositorySettings(page, "fixture/one");
    await editor.getByRole("switch", { name: "Monitor fixture/one" }).click();
    await expect(
      editor.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Monitoring: Enabled");
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor).toHaveCount(0);
    const authorized = (await store("snapshot")).settings;
    await page
      .getByRole("button", { name: "Set up with Genie", exact: true })
      .click();
    await expect(page.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "4",
    );
    await page.locator("[data-genie-next]").click();
    await expect(
      page.getByRole("button", { name: "Finish setup", exact: true }),
    ).toBeEnabled();
    await expect(page.locator("[data-genie-confirm]")).toHaveCount(0);
    await expect(
      page.getByText("All currently open and future matching PRs", {
        exact: true,
      }),
    ).toBeVisible();
    await page.screenshot({
      path: info.outputPath("genie-saved-configuration.png"),
    });
    await page
      .getByRole("button", { name: "Finish setup", exact: true })
      .click();
    expect((await store("snapshot")).settings).toEqual(authorized);
  });
}
