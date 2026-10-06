import { test, expect } from "./fixtures.mjs";
import {
  section,
  repositorySettings,
  chooseRepositoryAccount,
} from "./navigation.mjs";
import {
  providerFixture,
  repositoryPage,
  addByUrl,
  reviewer,
} from "./repository-provider-fixture.mjs";

for (const embedded of [true, false]) {
  for (const state of ["empty", "disconnected"]) {
    test(`R2 Manage Copilot accounts follows the actual button (${embedded ? "panel" : "standalone"}, ${state})`, async ({
      page,
      store,
    }) => {
      const settings = (await store("snapshot")).settings;
      settings.agents = state === "empty" ? [] : [reviewer];
      await store("seed_settings", settings);
      const fixture = await providerFixture(page, store, (command) => {
        if (command === "copilot_auth_state")
          return {
            accounts:
              state === "empty"
                ? []
                : [
                    {
                      provider: "copilot",
                      account_id: "33",
                      login: "ai-only",
                      state: "reconnect_required",
                      reason: "expired",
                    },
                  ],
            flow: { state: "idle" },
          };
      });
      await repositoryPage(page, store, embedded);
      await section(page, "Agents");
      if (state === "empty") {
        await expect(page.locator(".settings-empty")).toContainText(
          "repositories in Repositories",
        );
        await expect(page.locator(".settings-empty")).not.toContainText(
          "Integrations",
        );
      }
      await page
        .getByRole("button", { name: "Manage Copilot accounts", exact: true })
        .click();
      await expect(page.locator(".copilot-auth-card")).toBeVisible();
      await expect(page.locator(".repository-library")).toHaveCount(0);
      await expect(
        page.getByRole("button", {
          name:
            state === "empty"
              ? "Connect Copilot account"
              : "Reconnect Copilot ai-only",
          exact: true,
        }),
      ).toBeVisible();
      if (embedded)
        await expect(page.locator(".settings-window")).toHaveAttribute(
          "data-settings-section",
          "copilot",
        );
      else
        await expect(page.locator('[data-section="accounts"]')).toHaveAttribute(
          "aria-current",
          "page",
        );
      expect(
        fixture.calls.filter(({ command }) =>
          /^(start_|confirm_|disconnect_)/.test(command),
        ),
      ).toEqual([]);
    });
  }
}

for (const accountId of ["22", "44"]) {
  test(`NB1 editing A cannot silently jump to B's stable binding as ${accountId}`, async ({
    page,
    store,
  }) => {
    await providerFixture(page, store);
    await repositoryPage(page, store);
    const first = await addByUrl(page, "fixture/one", "22");
    await first
      .getByRole("button", { name: "Cancel repository changes" })
      .click();
    const second = await addByUrl(page, "fixture/two", accountId);
    await second
      .getByRole("button", { name: "Cancel repository changes" })
      .click();
    const before = (await store("snapshot")).settings;
    const original = await repositorySettings(page, "fixture/one");
    await original
      .getByText("Repository and connection", { exact: true })
      .click();
    await original
      .getByRole("button", { name: "Edit repository", exact: true })
      .click();
    const edit = page.getByRole("dialog", {
      name: "Edit repository",
      exact: true,
    });
    await edit.getByLabel("Repository URL").fill("fixture/two");
    await chooseRepositoryAccount(edit, accountId);
    await edit
      .getByRole("button", { name: "Add & configure", exact: true })
      .click();
    await expect(edit.getByRole("alert")).toContainText(
      "already configured on another row",
    );
    await expect(edit.getByLabel("Repository URL")).toHaveValue("fixture/two");
    await expect(
      page.getByRole("dialog", {
        name: "Settings for fixture/two",
        exact: true,
      }),
    ).toHaveCount(0);
    expect((await store("snapshot")).settings).toEqual(before);
    await edit.getByRole("button", { name: "Cancel", exact: true }).click();
    const reused = await addByUrl(page, "fixture/two", accountId);
    await expect(reused).toBeVisible();
    expect((await store("snapshot")).settings).toEqual(before);
  });
}

async function seedRepository(store, kind) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [structuredClone(reviewer)];
  if (kind === "unusable-agent") delete settings.agents[0].ai_account;
  settings.repositories = [
    {
      id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      provider: "github",
      name: "fixture/one",
      enabled: kind !== "reopened",
      provider_account_id: "22",
      provider_repository_id: "100",
      assignments: ["unusable-agent", "remove-last"].includes(kind)
        ? [
            {
              id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
              agent_id: reviewer.id,
              schedule: settings.defaults.schedule,
              comment: false,
            },
          ]
        : [],
    },
  ];
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

for (const kind of [
  "reopened",
  "legacy-enabled",
  "unusable-agent",
  "remove-last",
]) {
  test(`NB2 enabled repository Save validates usable assignments (${kind}) and preserves disabled repair`, async ({
    page,
    store,
  }) => {
    await seedRepository(store, kind);
    await providerFixture(page, store);
    await repositoryPage(page, store);
    let editor;
    if (kind === "reopened") editor = await addByUrl(page, "fixture/one");
    else editor = await repositorySettings(page, "fixture/one");
    const before = (await store("snapshot")).settings;
    if (kind === "remove-last")
      await editor
        .locator(".assignment-row")
        .getByRole("button", { name: "Remove", exact: true })
        .click();
    await editor.getByLabel("Enable repository monitoring on Save").check();
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor.locator("[data-resource-error]")).toContainText(
      kind === "unusable-agent"
        ? "explicit AI account and model"
        : "assign at least one saved Agent",
    );
    await expect(editor).toBeVisible();
    expect((await store("snapshot")).settings).toEqual(before);
    await editor.getByLabel("Enable repository monitoring on Save").uncheck();
    await editor
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(editor).toHaveCount(0);
    const repaired = (await store("snapshot")).settings;
    expect(repaired.repositories[0].enabled).toBe(false);
    expect(
      repaired.repository_authorizations[repaired.repositories[0].id],
    ).toBeNull();
    if (kind === "remove-last")
      expect(repaired.repositories[0].assignments ?? []).toEqual([]);
    await page.reload();
    expect((await store("snapshot")).settings).toEqual(repaired);
  });
}
