import { test, expect } from "./fixtures.mjs";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { section } from "./navigation.mjs";
import {
  providerFixture,
  repositoryPage,
  addByUrl,
  reviewer,
} from "./repository-provider-fixture.mjs";

const repoId = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const neighborId = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const assignment = {
  id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
  agent_id: reviewer.id,
  schedule: { kind: "interval", minutes: 5, timezone: "UTC" },
  comment: false,
  approve: false,
};

async function seed(store, assigned = true) {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  settings.repositories = [
    {
      id: repoId,
      provider: "github",
      name: "fixture/one",
      enabled: false,
      provider_account_id: "22",
      provider_repository_id: "100",
      assignments: assigned ? [assignment] : [],
    },
    {
      id: neighborId,
      provider: "github",
      name: "fixture/two",
      enabled: false,
      provider_account_id: "44",
      provider_repository_id: "200",
      assignments: [assignment],
    },
  ];
  await store("seed_settings", settings);
  return (await store("snapshot")).settings;
}

const toggle = (page) =>
  page.locator(`.repository-list [data-toggle-repository="${repoId}"]`);
const editor = (page) =>
  page.getByRole("dialog", { name: "Settings for fixture/one", exact: true });
const openEditor = async (page) => {
  await page.locator(`[data-repository="${repoId}"]`).click();
  return editor(page);
};

test("failed new configuration Save cannot enable a later nested assignment Save", async ({
  page,
  store,
  dataRoot,
}) => {
  const initial = await seed(store);
  initial.repositories = [initial.repositories[1]];
  await store("seed_settings", initial);
  await store("set_automation_paused", { paused: true });
  await providerFixture(page, store);
  await repositoryPage(page, store);
  await section(page, "Preferences");
  await page.locator("#global-capacity").fill("9");
  await section(page, "Repositories");
  const modal = await addByUrl(page);
  const monitoring = modal.getByRole("switch", { name: "Monitor fixture/one" });
  await expect(monitoring).toBeChecked();
  await modal.locator("[data-reviewer-trigger]").selectOption("off");
  const file = join(dataRoot, "config/settings.json");
  const before = await readFile(file);
  const intake = JSON.parse(before);
  const repository = intake.repositories.find((r) => r.name === "fixture/one");
  await modal
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(modal.locator("[data-resource-error]")).toContainText(
    "assign at least one saved Agent",
  );
  expect(await readFile(file)).toEqual(before);
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  await expect(modal.locator("[data-reviewer-trigger]")).toHaveValue("off");
  await modal
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assign = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await assign.getByLabel("Agent", { exact: true }).selectOption(reviewer.id);
  await assign
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await expect(assign).toHaveCount(0);
  const bytes = JSON.parse(await readFile(file, "utf8"));
  const committed = bytes.repositories.find((r) => r.id === repository.id);
  expect(committed.enabled).toBe(false);
  expect(committed.assignments).toHaveLength(1);
  expect(committed.assignments[0]).toMatchObject({
    agent_id: reviewer.id,
    comment: false,
  });
  expect(committed.overrides.reviewer_assignment).toBe(false);
  expect(bytes.repository_authorizations[repository.id]).toBeNull();
  expect((await store("snapshot")).settings).toEqual(bytes);
  expect(bytes.repositories.find((r) => r.id === neighborId)).toEqual(
    initial.repositories[0],
  );
  expect(bytes.defaults).toEqual(initial.defaults);
  expect(bytes.capacity).toBe(initial.capacity);
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  await expect(monitoring).toBeChecked();
  await expect(
    modal.locator("[data-repository-monitoring-detail]"),
  ).toContainText("Starts after you save valid configuration");
  await modal
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  const row = page.locator(`[data-repository="${repository.id}"]`);
  await expect(row.locator("[data-monitoring-state]")).toHaveText("Disabled");
  await expect(
    page.locator(`[data-toggle-repository="${repository.id}"]`),
  ).toHaveAttribute("aria-checked", "false");
  await row.click();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  await expect(monitoring).toBeChecked();
  await modal
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  const enabled = JSON.parse(await readFile(file, "utf8"));
  expect(enabled.repositories.find((r) => r.id === repository.id).enabled).toBe(
    true,
  );
  expect(enabled.repository_authorizations[repository.id]).toMatchObject({
    account_id: "22",
    repository_id: "100",
  });
  expect((await store("automation_snapshot")).paused).toBe(true);
  await expect(row.locator("[data-monitoring-state]")).toHaveText("Enabled");
  await row.click();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Enabled",
  );
  await expect(monitoring).toBeChecked();
  await modal
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await section(page, "Preferences");
  await expect(page.locator("#global-capacity")).toHaveValue("9");
});

test("nested assignment Save refreshes the editor and listing from actual saved monitoring", async ({
  page,
  store,
}) => {
  const initial = await seed(store, false);
  initial.repositories[0].enabled = true;
  await store("seed_settings", initial);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  const modal = await openEditor(page);
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Enabled",
  );
  await expect(
    modal.locator("[data-repository-monitoring-detail]"),
  ).toContainText("Needs setup");
  await modal
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assign = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await assign.getByLabel("Agent", { exact: true }).selectOption(reviewer.id);
  await assign
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await expect(assign).toHaveCount(0);
  const saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].enabled).toBe(true);
  expect(saved.repository_authorizations[repoId]).toMatchObject({
    account_id: "22",
    repository_id: "100",
  });
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Enabled",
  );
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).toBeChecked();
  await expect(
    modal.locator("[data-repository-monitoring-detail]"),
  ).not.toContainText("Needs setup");
  await modal
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await expect(page.locator(`[data-repository="${repoId}"]`)).not.toContainText(
    "Needs setup",
  );
});

for (const width of [280, 320]) {
  for (const failedAccount of [false, true]) {
    test(`short standalone ${width}x300 at 200% text keeps the full switch focus visible (${failedAccount ? "account error" : "global pause"})`, async ({
      page,
      store,
    }, info) => {
      const initial = await seed(store);
      await store("save_resource", {
        edit: {
          kind: "repository",
          id: repoId,
          expected: initial.repositories[0],
          value: { ...initial.repositories[0], enabled: true },
        },
      });
      await store("set_automation_paused", { paused: true });
      const fixture = await providerFixture(page, store);
      if (failedAccount) {
        fixture.accounts[0] = {
          ...fixture.accounts[0],
          state: "reconnect_required",
          reason: "expired",
        };
        await page.addInitScript(() => {
          const invoke = window.__TAURI_INTERNALS__.invoke;
          window.__TAURI_INTERNALS__.invoke = (command, args) =>
            command === "save_resource" && args.edit?.value?.enabled
              ? Promise.reject({
                  stage: "session",
                  account_id: "22",
                  error: "signed_out",
                })
              : invoke(command, args);
        });
      }
      await page.setViewportSize({ width, height: 300 });
      await page.emulateMedia({
        forcedColors: "active",
        reducedMotion: "reduce",
      });
      await repositoryPage(page, store, false);
      const list = page.locator(".repository-list");
      const open = page.locator(`[data-repository="${repoId}"]`);
      await expect(open).toContainText(
        failedAccount ? "Reconnect account" : "Global Monitoring paused",
      );
      const originalSize = await toggle(page).evaluate((node) =>
        parseFloat(getComputedStyle(node).fontSize),
      );
      const nextKey =
        page.context().browser().browserType().name() === "webkit" &&
        process.platform === "darwin"
          ? "Alt+Tab"
          : "Tab";
      const enlargeText = () =>
        list.evaluate((root) => {
          const sizes = [...root.querySelectorAll("*")].map((node) => [
            node,
            parseFloat(getComputedStyle(node).fontSize),
          ]);
          for (const [node, size] of sizes)
            node.style.fontSize = `${size * 2}px`;
        });
      const fullFocus = async (phase) => {
        await toggle(page).scrollIntoViewIfNeeded();
        await expect(toggle(page)).toBeFocused();
        const geometry = await toggle(page).evaluate((node) => {
          const rect = node.getBoundingClientRect();
          const style = getComputedStyle(node);
          const outline =
            parseFloat(style.outlineWidth) +
            Math.max(0, parseFloat(style.outlineOffset));
          const focus = {
            left: rect.left - outline,
            right: rect.right + outline,
            top: rect.top - outline,
            bottom: rect.bottom + outline,
          };
          const clips = [];
          for (
            let parent = node.parentElement;
            parent;
            parent = parent.parentElement
          ) {
            const overflow = getComputedStyle(parent);
            const clipsX = ["auto", "scroll", "hidden", "clip"].includes(
              overflow.overflowX,
            );
            const clipsY = ["auto", "scroll", "hidden", "clip"].includes(
              overflow.overflowY,
            );
            if (clipsX || clipsY) {
              const bounds = parent.getBoundingClientRect();
              clips.push({
                name: `${parent.tagName}#${parent.id}.${parent.className}`,
                clipsX,
                clipsY,
                left: bounds.left + parent.clientLeft,
                right: bounds.left + parent.clientLeft + parent.clientWidth,
                top: bounds.top + parent.clientTop,
                bottom: bounds.top + parent.clientTop + parent.clientHeight,
                clientHeight: parent.clientHeight,
              });
            }
          }
          return {
            browserViewport: { width: innerWidth, height: innerHeight },
            control: {
              top: rect.top,
              bottom: rect.bottom,
              height: rect.height,
            },
            fontSize: parseFloat(style.fontSize),
            outline,
            focus,
            clips,
          };
        });
        await writeFile(
          info.outputPath(`${phase}-geometry.json`),
          JSON.stringify({ ...geometry, nextKey }, null, 2),
        );
        await page.screenshot({ path: info.outputPath(`${phase}-focus.png`) });
        expect(geometry.fontSize).toBe(originalSize * 2);
        expect(geometry.outline).toBeGreaterThan(0);
        expect(geometry.focus.left).toBeGreaterThanOrEqual(0);
        expect(geometry.focus.right).toBeLessThanOrEqual(width);
        expect(geometry.focus.top).toBeGreaterThanOrEqual(0);
        expect(geometry.focus.bottom).toBeLessThanOrEqual(300);
        expect(geometry.clips.length).toBeGreaterThan(0);
        for (const clip of geometry.clips) {
          if (clip.clipsX) {
            expect(geometry.focus.left, clip.name).toBeGreaterThanOrEqual(
              clip.left,
            );
            expect(geometry.focus.right, clip.name).toBeLessThanOrEqual(
              clip.right,
            );
          }
          if (clip.clipsY) {
            expect(geometry.focus.top, clip.name).toBeGreaterThanOrEqual(
              clip.top,
            );
            expect(geometry.focus.bottom, clip.name).toBeLessThanOrEqual(
              clip.bottom,
            );
          }
        }
      };
      await enlargeText();
      await open.focus();
      await page.keyboard.press(nextKey);
      await fullFocus("enabled");
      await page.keyboard.press("Space");
      await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
      await enlargeText();
      await fullFocus("disabled");
      await page.keyboard.press("Space");
      if (failedAccount) {
        const error = page.locator("#error");
        await expect(error).toContainText(
          "Connect the PR Sniper GitHub OAuth App",
        );
        await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
        await error.scrollIntoViewIfNeeded();
        await expect(error).toBeInViewport();
        await page.screenshot({
          path: info.outputPath("actionable-error.png"),
        });
      } else {
        await expect(toggle(page)).toHaveAttribute("aria-checked", "true");
        await expect(open).toContainText("Global Monitoring paused");
      }
      await enlargeText();
      await fullFocus(failedAccount ? "rejected-enable" : "paused-enable");
      const saved = (await store("snapshot")).settings;
      expect(saved.repositories[0].enabled).toBe(!failedAccount);
      expect(saved.repository_authorizations[repoId]).toEqual(
        failedAccount
          ? null
          : expect.objectContaining({ account_id: "22", repository_id: "100" }),
      );
      expect((await store("automation_snapshot")).paused).toBe(true);
      expect(saved.repositories[1]).toEqual(initial.repositories[1]);
      expect(
        fixture.calls.some((call) =>
          /resolve|preview|activation|pull_requests/.test(call.command),
        ),
      ).toBe(false);
      await open.click();
      await expect(
        editor(page).locator("[data-repository-monitoring-state]"),
      ).toHaveText(failedAccount ? "Disabled" : "Enabled");
      await editor(page)
        .getByRole("button", { name: "Close dialog", exact: true })
        .click();
      await expect(open).toBeFocused();
    });
  }
}

for (const embedded of [true, false]) {
  for (const width of [320, 408]) {
    for (const textScale of [1, 2]) {
      test(`repository state and switch remain contained at ${width}px with ${textScale * 100}% actual text (${embedded ? "panel" : "standalone"})`, async ({
        page,
        store,
      }, info) => {
        await page.setViewportSize({ width, height: 744 });
        await page.emulateMedia({
          forcedColors: "active",
          reducedMotion: "reduce",
        });
        await seed(store);
        await store("set_automation_paused", { paused: true });
        await providerFixture(page, store);
        await repositoryPage(page, store, embedded);
        const list = page.locator(".repository-list");
        const originalControlSize = await toggle(page).evaluate((control) =>
          parseFloat(getComputedStyle(control).fontSize),
        );
        const enlargeText = () =>
          list.evaluate((root, scale) => {
            const sizes = [...root.querySelectorAll("*")].map((element) => ({
              element,
              size: parseFloat(getComputedStyle(element).fontSize),
            }));
            for (const { element, size } of sizes)
              element.style.fontSize = `${size * scale}px`;
          }, textScale);
        await enlargeText();
        const contained = () =>
          list.evaluate((root) => {
            const bounds = root.getBoundingClientRect();
            const inside = (rect) =>
              rect.left >= bounds.left &&
              rect.right <= bounds.right &&
              rect.top >= bounds.top &&
              rect.bottom <= bounds.bottom;
            const rows = [
              ...root.querySelectorAll(".repository-monitoring-row"),
            ];
            return rows.map((row) => {
              const open = row.querySelector("[data-repository]");
              const copy = row.querySelector(".settings-row-copy");
              const state = row.querySelector("[data-monitoring-state]");
              const control = row.querySelector("[data-toggle-repository]");
              const rowBounds = row.getBoundingClientRect();
              const controlBounds = control.getBoundingClientRect();
              const text = document.createRange();
              text.selectNodeContents(state);
              const controlText = document.createRange();
              controlText.selectNodeContents(control);
              const copyBounds = copy.getBoundingClientRect();
              const copyText = [...copy.querySelectorAll("strong,small")].every(
                (element) => {
                  const range = document.createRange();
                  range.selectNodeContents(element);
                  return [...range.getClientRects()].every(
                    (rect) =>
                      inside(rect) &&
                      rect.left >= copyBounds.left &&
                      rect.right <= copyBounds.right,
                  );
                },
              );
              return {
                row: inside(row.getBoundingClientRect()),
                open: inside(open.getBoundingClientRect()),
                state: [...text.getClientRects()].every(inside),
                control: inside(control.getBoundingClientRect()),
                controlText: [...controlText.getClientRects()].every(inside),
                readableState: state.scrollWidth <= state.clientWidth,
                readableControl: control.scrollWidth <= control.clientWidth,
                copyText,
                fontSize: parseFloat(getComputedStyle(control).fontSize),
                extents: {
                  listLeft: bounds.left,
                  listRight: bounds.right,
                  rowLeft: rowBounds.left,
                  rowRight: rowBounds.right,
                  switchLeft: controlBounds.left,
                  switchRight: controlBounds.right,
                },
              };
            });
          });
        const geometry = await contained();
        expect(geometry).toHaveLength(2);
        const measurements = info.outputPath("containment.json");
        await writeFile(
          measurements,
          JSON.stringify(
            {
              browser: page.context().browser().browserType().name(),
              width,
              textScale,
              originalControlSize,
              geometry,
            },
            null,
            2,
          ),
        );
        await info.attach("containment.json", {
          path: measurements,
          contentType: "application/json",
        });
        for (const row of geometry) {
          expect(row).toMatchObject({
            row: true,
            open: true,
            state: true,
            control: true,
            controlText: true,
            readableState: true,
            readableControl: true,
            copyText: true,
          });
          expect(row.fontSize).toBe(originalControlSize * textScale);
        }
        await expect(toggle(page)).toHaveAccessibleName("Monitor fixture/one");
        await toggle(page).scrollIntoViewIfNeeded();
        await toggle(page).focus();
        await expect(toggle(page)).toBeFocused();
        const target = await toggle(page).boundingBox();
        expect(target.x).toBeGreaterThanOrEqual(0);
        expect(target.x + target.width).toBeLessThanOrEqual(width);
        expect(target.y).toBeGreaterThanOrEqual(0);
        expect(target.y + target.height).toBeLessThanOrEqual(744);
        await page.screenshot({
          path: info.outputPath("contained-text-and-focus.png"),
        });
        await page.keyboard.press("Space");
        await expect(toggle(page)).toHaveAttribute("aria-checked", "true");
        await expect(toggle(page)).toBeFocused();
        await expect(
          page.locator(`[data-repository="${repoId}"]`),
        ).toContainText("Global Monitoring paused");
        expect((await store("automation_snapshot")).paused).toBe(true);
        await enlargeText();
        for (const row of await contained()) {
          expect(row).toMatchObject({
            row: true,
            open: true,
            state: true,
            control: true,
            controlText: true,
            readableState: true,
            readableControl: true,
            copyText: true,
          });
          expect(row.fontSize).toBe(originalControlSize * textScale);
        }
        await page.locator(`[data-repository="${repoId}"]`).click();
        await expect(editor(page)).toBeVisible();
        await expect(
          editor(page).locator("[data-repository-monitoring-state]"),
        ).toHaveText("Enabled");
      });
    }
  }
}

for (const embedded of [true, false]) {
  test(`persistent repository switch keeps global pause separate (${embedded ? "panel" : "standalone"})`, async ({
    page,
    store,
  }, info) => {
    const original = await seed(store);
    await store("set_automation_paused", { paused: true });
    const fixture = await providerFixture(page, store);
    await repositoryPage(page, store, embedded);
    await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
    await toggle(page).focus();
    await page.keyboard.press("Space");
    await expect(toggle(page)).toHaveAttribute("aria-checked", "true");
    await expect(toggle(page)).toBeFocused();
    await expect(page.locator(`[data-repository="${repoId}"]`)).toContainText(
      "Global Monitoring paused",
    );
    let saved = (await store("snapshot")).settings;
    expect(saved.repositories[0].enabled).toBe(true);
    expect(saved.repository_authorizations[repoId]).toMatchObject({
      account_id: "22",
      repository_id: "100",
    });
    expect(saved.repositories[1]).toEqual(original.repositories[1]);
    expect(saved.agents).toEqual(original.agents);
    expect(saved.repositories[0].assignments[0].comment).toBe(false);
    expect((await store("automation_snapshot")).paused).toBe(true);
    const modal = await openEditor(page);
    await expect(
      modal.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Enabled");
    await expect(modal.getByText(/on Save/)).toHaveCount(0);
    await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
    await expect(
      modal.locator("[data-repository-monitoring-state]"),
    ).toHaveText("Disabled");
    saved = (await store("snapshot")).settings;
    expect(saved.repositories[0].enabled).toBe(false);
    expect(saved.repository_authorizations[repoId]).toBeNull();
    await modal
      .getByRole("button", { name: "Save repository", exact: true })
      .click();
    await expect(modal).toHaveCount(0);
    expect((await store("automation_snapshot")).paused).toBe(true);
    await page.reload();
    if (embedded)
      await page
        .getByRole("navigation", { name: "Application destinations" })
        .getByRole("button", { name: "Settings", exact: true })
        .click();
    await section(page, "Repositories");
    await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
    await page.emulateMedia({
      forcedColors: "active",
      reducedMotion: "reduce",
    });
    await page.setViewportSize({ width: 320, height: 300 });
    await toggle(page).focus();
    await expect(toggle(page)).toBeFocused();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: info.outputPath("disabled-compact-high-contrast.png"),
    });
    expect(
      fixture.calls.some((call) =>
        /preview|activation|pull_requests/.test(call.command),
      ),
    ).toBe(false);
  });
}

test("quick editor switch commits only saved configuration and retains repository and Preferences drafts", async ({
  page,
  store,
}) => {
  const original = await seed(store);
  await providerFixture(page, store);
  await repositoryPage(page, store, false);
  await section(page, "Preferences");
  await page.locator("#global-cron").fill("invalid draft cron");
  await page.locator("#global-capacity").fill("9");
  await section(page, "Repositories");
  const modal = await openEditor(page);
  await modal.locator("[data-reviewer-trigger]").selectOption("off");
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Enabled",
  );
  await expect(modal.locator("[data-reviewer-trigger]")).toHaveValue("off");
  let saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].overrides?.reviewer_assignment).toBeUndefined();
  expect(saved.defaults).toEqual(original.defaults);
  expect(saved.capacity).toBe(original.capacity);
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  await modal
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  saved = (await store("snapshot")).settings;
  expect(saved.repositories[0].enabled).toBe(false);
  expect(saved.repositories[0].overrides.reviewer_assignment).toBe(false);
  expect(saved.repositories[1]).toEqual(original.repositories[1]);
  await section(page, "Preferences");
  await expect(page.locator("#global-cron")).toHaveValue("invalid draft cron");
  await expect(page.locator("#global-capacity")).toHaveValue("9");
});

test("invalid quick enable reports failure without optimistic enabled state or losing row focus", async ({
  page,
  store,
}) => {
  const original = await seed(store, false);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  await toggle(page).focus();
  await page.keyboard.press("Space");
  await expect(page.locator("#error")).toContainText(
    "assign at least one saved Agent",
  );
  await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
  await expect(toggle(page)).toBeFocused();
  expect((await store("snapshot")).settings).toEqual(original);
  const modal = await openEditor(page);
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-resource-error]")).toContainText(
    "assign at least one saved Agent",
  );
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  await expect(modal.locator("[data-repository-monitoring-state]")).toHaveText(
    "Disabled",
  );
  expect((await store("snapshot")).settings).toEqual(original);
});

test("compare-save conflicts retain the repository draft and never overwrite newer saved state", async ({
  page,
  store,
}) => {
  const original = await seed(store);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  const modal = await openEditor(page);
  await modal.locator("[data-reviewer-trigger]").selectOption("off");
  const newer = structuredClone(original.repositories[0]);
  newer.watched_authors = [{ id: "11", login: "newer-author" }];
  await store("save_resource", {
    edit: {
      kind: "repository",
      id: repoId,
      expected: original.repositories[0],
      value: newer,
    },
  });
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).click();
  await expect(modal.locator("[data-resource-error]")).toContainText(
    "Resource changed",
  );
  await expect(modal.locator("[data-reviewer-trigger]")).toHaveValue("off");
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  expect((await store("snapshot")).settings.repositories[0]).toEqual(newer);
});

test("account loss stays separate from enablement and allows a durable off switch without provider requests", async ({
  page,
  store,
}) => {
  const original = await seed(store);
  await store("save_resource", {
    edit: {
      kind: "repository",
      id: repoId,
      expected: original.repositories[0],
      value: { ...original.repositories[0], enabled: true },
    },
  });
  const fixture = await providerFixture(page, store);
  fixture.accounts[0] = {
    ...fixture.accounts[0],
    state: "reconnect_required",
    reason: "expired",
  };
  await page.addInitScript(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => {
      if (command === "save_resource" && args.edit?.value?.enabled)
        return Promise.reject({
          stage: "session",
          account_id: "22",
          error: "signed_out",
        });
      return invoke(command, args);
    };
  });
  await repositoryPage(page, store);
  const row = page.locator(`[data-repository="${repoId}"]`);
  await expect(row).toContainText("Enabled");
  await expect(row).toContainText("Reconnect account");
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(true);
  await toggle(page).click();
  await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await toggle(page).click();
  await expect(page.locator("#error")).toContainText(
    "Connect the PR Sniper GitHub OAuth App",
  );
  await expect(toggle(page)).toHaveAttribute("aria-checked", "false");
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  expect(
    fixture.calls.some((call) =>
      /resolve|preview|activation/.test(call.command),
    ),
  ).toBe(false);
});

test("new rows stay disabled until valid Save and an explicit off choice survives assignment saves and reopening", async ({
  page,
  store,
}) => {
  const settings = (await store("snapshot")).settings;
  settings.agents = [reviewer];
  await store("seed_settings", settings);
  await providerFixture(page, store);
  await repositoryPage(page, store);
  let modal = await addByUrl(page);
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).toBeChecked();
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await modal.getByRole("switch", { name: "Monitor fixture/one" }).uncheck();
  await modal
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  const assign = page.getByRole("dialog", {
    name: "Assign agent",
    exact: true,
  });
  await assign.getByLabel("Agent", { exact: true }).selectOption(reviewer.id);
  await assign
    .getByRole("button", { name: "Assign agent", exact: true })
    .click();
  await expect(assign).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
  await modal
    .getByRole("button", { name: "Close dialog", exact: true })
    .click();
  await page.locator("[data-repository]").click();
  modal = editor(page);
  await expect(
    modal.getByRole("switch", { name: "Monitor fixture/one" }),
  ).not.toBeChecked();
  await modal
    .getByRole("button", { name: "Save repository", exact: true })
    .click();
  await expect(modal).toHaveCount(0);
  expect((await store("snapshot")).settings.repositories[0].enabled).toBe(
    false,
  );
});
