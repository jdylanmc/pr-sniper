import { test, expect } from "./fixtures.mjs";
import {
  providerFixture,
  repositoryPage,
} from "./repository-provider-fixture.mjs";

const AVERY = "00000000-0000-0000-0000-000000000001";
const id = (number) =>
  `00000000-0000-4000-8000-${String(number).padStart(12, "0")}`;
const github = (account_id, login, state = "connected") => ({
  provider: "github",
  account_id,
  login,
  state,
});
const accounts = [
  github("22", "fixture"),
  github("44", "other"),
  github("55", "lapsed", "reconnect_required"),
];
const adoSetup = {
  cli: { state: "ready", version: "2.90.0" },
  minimum: "2.54.0",
  installer: "homebrew",
  organization: {
    state: "tenant_known",
    name: "contoso",
    tenant: "00000000-0000-0000-0000-00000000000a",
  },
  account: {
    state: "connected",
    identity: {
      id: AVERY,
      display_name: "Avery Example",
      unique_name: "avery@contoso.example",
    },
  },
  checking: false,
};
const repo = (number, provider, name, account) => ({
  id: id(number),
  provider,
  name,
  enabled: false,
  provider_account_id: account,
  provider_repository_id: String(100 + number),
});

async function seed(store) {
  const settings = (await store("snapshot")).settings;
  settings.repositories = [
    repo(1, "azure_devops", "contoso/project/repo", AVERY),
    repo(2, "github", "octo-org/app", "22"),
    repo(3, "github", "octo-org/legacy", "99"),
    repo(4, "github", "octo-org/web", "22"),
    repo(5, "github", "octo-org/paused", "55"),
  ];
  await store("seed_settings", settings);
}

async function open(page, store, setup = adoSetup) {
  const fixture = await providerFixture(page, store);
  fixture.accounts = accounts;
  await page.addInitScript((setup) => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args = {}) =>
      command.startsWith("azure_devops_")
        ? window.__settingsTrack(Promise.resolve(setup))
        : original(command, args);
  }, setup);
  await repositoryPage(page, store);
  await page.evaluate(() => window.__settingsIdle());
}

const sectionOf = (page, name) =>
  page.getByRole("region", { name, exact: true });

test("repositories group into one section per account, providers first then unavailable", async ({
  page,
  store,
}, info) => {
  await page.setViewportSize({ width: 408, height: 744 });
  await seed(store);
  await open(page, store);
  await page.screenshot({ path: info.outputPath("repository-sections.png") });
  const sections = page.locator("[data-repository-account]");
  await expect(sections).toHaveCount(5);
  expect(
    await sections.evaluateAll((nodes) =>
      nodes.map((node) => [
        node.dataset.provider,
        node.querySelector("h3").textContent.replace(/\s+/g, " ").trim(),
        "repositoryDisconnected" in node.dataset,
        [...node.querySelectorAll("[data-repository]")].map((row) =>
          row.querySelector("strong").textContent.trim(),
        ),
      ]),
    ),
  ).toEqual([
    ["github", "GitHub fixture", false, ["octo-org/app", "octo-org/web"]],
    ["github", "GitHub other", false, []],
    [
      "azure_devops",
      "Azure DevOps / contoso Avery Example",
      false,
      ["contoso/project/repo"],
    ],
    ["github", "GitHub lapsed", true, ["octo-org/paused"]],
    ["github", "GitHub Unavailable", true, ["octo-org/legacy"]],
  ]);
});

test("each account section carries its own add action, including empty and Azure DevOps accounts", async ({
  page,
  store,
}) => {
  await seed(store);
  await open(page, store);
  const empty = sectionOf(page, "GitHub other");
  await expect(empty.locator("[data-repository]")).toHaveCount(0);
  await expect(
    empty.getByRole("button", {
      name: "Browse repositories as other",
      exact: true,
    }),
  ).toBeEnabled();
  const azure = sectionOf(page, "Azure DevOps / contoso Avery Example");
  await expect(azure).toHaveAttribute("data-provider", "azure_devops");
  await expect(
    azure.getByRole("button", {
      name: "Browse repositories as Avery Example",
      exact: true,
    }),
  ).toBeEnabled();
  const github = sectionOf(page, "GitHub fixture");
  await expect(github.locator(".provider-mark")).toHaveText("GH");
  await expect(azure.locator(".provider-mark")).toHaveText("AZ");
  // The header names provider and account; rows no longer repeat them.
  await expect(github.locator("[data-repository]").first()).not.toContainText(
    "GitHub /",
  );
  await expect(azure.locator("[data-repository]")).not.toContainText(
    "Azure DevOps /",
  );
  const lapsed = sectionOf(page, "GitHub lapsed");
  await expect(lapsed).toHaveAttribute("data-repository-disconnected", "");
  await expect(lapsed).toContainText("Reconnect account");
  await expect(
    lapsed.getByRole("button", {
      name: "Browse repositories as lapsed",
      exact: true,
    }),
  ).toBeDisabled();
  await github
    .getByRole("button", { name: "Browse repositories as fixture" })
    .click();
  await expect(
    page.getByRole("dialog", {
      name: "Browse repositories as fixture",
      exact: true,
    }),
  ).toBeVisible();
});

test("without Azure DevOps, only GitHub sections show and unknown repos stay at the bottom", async ({
  page,
  store,
}) => {
  await seed(store);
  await open(page, store, {
    ...adoSetup,
    account: { state: "signed_out" },
  });
  await expect(
    page.locator('[data-repository-account][data-provider="azure_devops"] h3'),
  ).toHaveText(["Azure DevOps Unavailable"]);
  await expect(page.locator("[data-browse-azure]")).toHaveCount(0);
  await expect(
    page.locator("[data-repository-account]").last(),
  ).toHaveAttribute("data-repository-disconnected", "");
});

// Many rows across accounts so the list scrolls well past the + URL pill.
async function seedMany(store) {
  const settings = (await store("snapshot")).settings;
  settings.repositories = [
    repo(1, "azure_devops", "contoso/project/repo", AVERY),
    ...Array.from({ length: 14 }, (_, i) =>
      repo(10 + i, "github", `octo-org/app-${i}`, "22"),
    ),
    ...Array.from({ length: 8 }, (_, i) =>
      repo(30 + i, "github", `octo-team/svc-${i}`, "44"),
    ),
    repo(5, "github", "octo-org/paused", "55"),
  ];
  await store("seed_settings", settings);
}

// Accessible names of row/header controls whose visible box the pill's box
// intersects. Visible = clipped by scrolling ancestors and by the pill's own
// band when that band is an opaque sticky toolbar that rows scroll behind.
const coveredControls = (page) =>
  page.evaluate(() => {
    const element = document.querySelector("#add-repository");
    const pill = element.getBoundingClientRect();
    const hit = document.elementFromPoint(
      pill.left + pill.width / 2,
      pill.top + pill.height / 2,
    );
    if (!element.contains(hit)) return ["+ URL is not on top"];
    const band = element.parentElement;
    const bandStyle = getComputedStyle(band);
    const bandTop =
      bandStyle.position === "sticky" &&
      !/rgba\(.*, 0\)|transparent/.test(bandStyle.backgroundColor)
        ? band.getBoundingClientRect().top
        : Infinity;
    const visible = (control) => {
      const r = control.getBoundingClientRect();
      let { top, bottom } = r;
      for (let el = control.parentElement; el; el = el.parentElement)
        if (
          /^(auto|scroll|hidden|clip)$/.test(getComputedStyle(el).overflowY)
        ) {
          const box = el.getBoundingClientRect();
          top = Math.max(top, box.top);
          bottom = Math.min(bottom, box.bottom);
        }
      return {
        left: r.left,
        right: r.right,
        top,
        bottom: Math.min(bottom, bandTop),
      };
    };
    return [
      ...document.querySelectorAll(
        "[data-repository-accounts] :is(button, [role=switch])",
      ),
    ]
      .map((control) => [control, visible(control)])
      .filter(
        ([, r]) =>
          r.right > r.left &&
          r.bottom > r.top &&
          r.left < pill.right &&
          r.right > pill.left &&
          r.top < pill.bottom &&
          r.bottom > pill.top,
      )
      .map(([control]) => control.getAttribute("aria-label"));
  });

const scrollTo = (page, fraction) =>
  page.locator("[data-repository-accounts]").evaluate((node, fraction) => {
    for (let el = node; el; el = el.parentElement)
      if (
        el.scrollHeight > el.clientHeight &&
        /^(auto|scroll)$/.test(getComputedStyle(el).overflowY)
      ) {
        el.scrollTop = Math.round(
          (el.scrollHeight - el.clientHeight) * fraction,
        );
        return el.scrollTop;
      }
    return -1;
  }, fraction);

for (const theme of ["light", "dark"])
  for (const [width, height] of [
    [408, 744],
    [408, 520],
    [720, 600],
  ])
    test(`the + URL pill never covers a row or header control while scrolling (${theme}, ${width}x${height})`, async ({
      page,
      store,
    }, info) => {
      await page.setViewportSize({ width, height });
      await seedMany(store);
      await open(page, store);
      await page.evaluate((theme) => {
        document.documentElement.dataset.theme = theme;
      }, theme);
      const pill = page.getByRole("button", {
        name: "Add repository by URL",
        exact: true,
      });
      await expect(pill).toBeVisible();
      const scrolled = [];
      for (const fraction of [0, 0.2, 0.37, 0.5, 0.63, 0.8, 1]) {
        scrolled.push(await scrollTo(page, fraction));
        expect(await coveredControls(page), `at ${fraction}`).toEqual([]);
      }
      // The sweep really moved the list under the pill's band.
      expect(scrolled.at(-1)).toBeGreaterThan(0);
      await page.screenshot({
        path: info.outputPath(`repository-sections-end-${theme}.png`),
      });
      // Every toggle stays reachable: the last row is fully above the pill.
      const last = page.locator("[data-toggle-repository]").last();
      await expect(last).toBeInViewport({ ratio: 1 });
      await last.click({ trial: true });
      // Keyboard: + URL keeps its name and opens the add dialog.
      await pill.focus();
      await page.keyboard.press("Enter");
      await expect(page.getByRole("dialog")).toBeVisible();
    });

for (const [width, height] of [
  [408, 744],
  [408, 520],
])
  test(`tabbing through a long list never leaves focus behind the + URL band (${width}x${height})`, async ({
    page,
    store,
  }) => {
    const nextKey =
      page.context().browser().browserType().name() === "webkit" &&
      process.platform === "darwin"
        ? "Alt+Tab"
        : "Tab";
    await page.setViewportSize({ width, height });
    await seedMany(store);
    await open(page, store);
    const controls = page.locator(
      "[data-repository-accounts] :is(button, [role=switch]):not([disabled])",
    );
    const total = await controls.count();
    await controls.first().focus();
    const hidden = [];
    for (let i = 1; i < total; i++) {
      await page.keyboard.press(nextKey);
      const result = await page.evaluate(() => {
        const focused = document.activeElement;
        if (!focused?.closest("[data-repository-accounts]")) return null;
        const band = document
          .querySelector("#add-repository")
          .parentElement.getBoundingClientRect();
        const scroller = document
          .querySelector("#content")
          .getBoundingClientRect();
        const box = focused.getBoundingClientRect();
        return box.bottom <= band.top && box.top >= scroller.top
          ? ""
          : focused.getAttribute("aria-label");
      });
      if (result) hidden.push(result);
    }
    expect(hidden).toEqual([]);
  });
