import { expect, test } from "./fixtures.mjs";

const accounts = [
  {
    provider: "github",
    state: "connected",
    account_id: "102617352",
    login: "dylanmccurry-msft",
  },
  {
    provider: "github",
    state: "connected",
    account_id: "6954990",
    login: "jdylanmc",
  },
];
const connecting = {
  state: "connecting",
  user_code: "ABCD-EFGH",
  verification_uri: "https://github.com/login/device",
};

async function mockAuth(
  page,
  flow,
  clipboardFailure = false,
  holdClipboard = false,
) {
  await page.addInitScript(
    ({ accounts, flow, clipboardFailure, holdClipboard }) => {
      window.__authView = { accounts, flow };
      window.__authReads = 0;
      window.__copiedCodes = [];
      window.__copyRequests = 0;
      window.__clipboardGate = holdClipboard ? Promise.withResolvers() : null;
      window.__clipboardFailure = clipboardFailure;
      Object.defineProperty(navigator, "clipboard", {
        value: {
          async writeText(value) {
            window.__copyRequests += 1;
            if (window.__clipboardGate) await window.__clipboardGate.promise;
            if (window.__clipboardFailure)
              throw new DOMException("Clipboard denied", "NotAllowedError");
            window.__copiedCodes.push(value);
          },
        },
      });
      const original = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command === "github_auth_state") {
          window.__authReads += 1;
          return Promise.resolve(window.__authView);
        }
        if (command === "cancel_github_auth") {
          window.__authView.flow = { state: "idle" };
          return Promise.resolve(window.__authView);
        }
        if (command === "list_provider_repositories")
          return Promise.resolve({
            identity: { id: "102617352", login: "dylanmccurry-msft" },
            repositories: [
              {
                id: "42",
                name: `long-organization-name/${"repository-name-".repeat(5)}`,
              },
            ],
          });
        return original(command, args);
      };
    },
    { accounts, flow, clipboardFailure, holdClipboard },
  );
}

async function expectContained(page) {
  const overflow = await page.locator(".github-auth-card").evaluate((card) => {
    const content = document.querySelector("#content");
    const bounds = card.getBoundingClientRect();
    return {
      card: card.scrollWidth - card.clientWidth,
      content: content.scrollWidth - content.clientWidth,
      clippedControls: [...card.querySelectorAll("button, code")].filter(
        (control) => {
          const rect = control.getBoundingClientRect();
          return (
            rect.left < bounds.left ||
            rect.right > bounds.right ||
            control.scrollWidth > control.clientWidth + 1
          );
        },
      ).length,
    };
  });
  expect(overflow).toEqual({ card: 0, content: 0, clippedControls: 0 });
}

for (const viewport of [
  { width: 1120, height: 728 },
  { width: 780, height: 560 },
  { width: 390, height: 600 },
]) {
  test(`accounts and sign-in actions fit at ${viewport.width}px`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await mockAuth(page, { state: "idle" });
    await page.goto("/?view=settings");
    const card = page.locator(".github-auth-card");
    await expect(card).toContainText("2 GitHub accounts");
    await expectContained(page);
    await card
      .getByRole("button", { name: "Load repositories for dylanmccurry-msft" })
      .click();
    await expect(card).toContainText("long-organization-name/");
    await expectContained(page);
  });

  test(`one-time code and Copy code fit at ${viewport.width}px`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await mockAuth(page, connecting);
    await page.goto("/?view=settings");
    const card = page.locator(".github-auth-card");
    await expect(card).toContainText("ABCD-EFGH");
    await expectContained(page);
    const copy = card.getByRole("button", { name: /Copy.*code/ });
    await copy.scrollIntoViewIfNeeded();
    await expect(copy).toBeInViewport({ ratio: 1 });
    await expect(
      card.locator("code").filter({ hasText: "ABCD-EFGH" }),
    ).toBeInViewport({ ratio: 1 });
  });
}

test("copy feedback and keyboard focus survive unchanged auth polling", async ({
  page,
}) => {
  await mockAuth(page, connecting);
  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  const copy = card.getByRole("button", { name: /Copy.*code/ });
  await copy.focus();
  await page.keyboard.press("Enter");
  await expect
    .poll(() => page.evaluate(() => window.__copiedCodes))
    .toEqual(["ABCD-EFGH"]);
  const reads = await page.evaluate(() => window.__authReads);
  await expect
    .poll(() => page.evaluate(() => window.__authReads))
    .toBeGreaterThan(reads + 3);
  await expect(card).toContainText("Code copied");
  await expect(copy).toBeFocused();
  await expect(copy).toHaveText("Copy code");

  await page.evaluate(() => {
    window.__authView.flow = {
      ...window.__authView.flow,
      user_code: "JKLM-NOPQ",
    };
  });
  await expect(card).toContainText("JKLM-NOPQ");
  await expect(card).not.toContainText("Code copied");
  await copy.click();
  await expect
    .poll(() => page.evaluate(() => window.__copiedCodes))
    .toEqual(["ABCD-EFGH", "JKLM-NOPQ"]);
  await card.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(card).not.toContainText("JKLM-NOPQ");
  await expect(card).not.toContainText("Code copied");
  await expect(copy).toHaveCount(0);
});

test("clipboard denial leaves the code selectable and explains manual copying", async ({
  page,
}) => {
  await mockAuth(page, connecting, true);
  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  const copy = card.getByRole("button", { name: /Copy.*code/ });
  await copy.click();
  const reads = await page.evaluate(() => window.__authReads);
  await expect
    .poll(() => page.evaluate(() => window.__authReads))
    .toBeGreaterThan(reads + 3);
  await expect(card.getByRole("alert")).toContainText(
    "Could not copy. Select the code and copy it manually.",
  );
  const code = card.locator("code").filter({ hasText: "ABCD-EFGH" });
  await expect(code).toBeVisible();
  expect(
    await code.evaluate((element) => getComputedStyle(element).userSelect),
  ).not.toBe("none");
  expect(await page.evaluate(() => window.__copiedCodes)).toEqual([]);
  await page.evaluate(() => (window.__clipboardFailure = false));
  await copy.click();
  await expect(card.getByRole("alert")).toHaveCount(0);
  await expect(card).toContainText("Code copied");
});

for (const clipboardFailure of [false, true]) {
  test(`asynchronous clipboard ${clipboardFailure ? "denial" : "success"} preserves focus and prevents duplicate copying`, async ({
    page,
  }) => {
    await mockAuth(page, connecting, clipboardFailure, true);
    await page.goto("/?view=settings");
    const card = page.locator(".github-auth-card");
    const copy = card.getByRole("button", { name: "Copy code" });
    await copy.focus();
    await page.keyboard.press("Enter");
    await expect.poll(() => page.evaluate(() => window.__copyRequests)).toBe(1);
    const reads = await page.evaluate(() => window.__authReads);
    await expect
      .poll(() => page.evaluate(() => window.__authReads))
      .toBeGreaterThan(reads + 3);
    await expect(copy).toBeFocused();
    await page.keyboard.press("Enter");
    expect(await page.evaluate(() => window.__copyRequests)).toBe(1);

    await page.evaluate(() => window.__clipboardGate.resolve());
    await expect(card).toContainText(
      clipboardFailure ? "Could not copy" : "Code copied",
    );
    await expect(copy).toBeFocused();
    await expect(copy).not.toHaveAttribute("aria-disabled", "true");
  });
}

test("a late clipboard reply cannot restore cancelled code or steal focus", async ({
  page,
}) => {
  await mockAuth(page, connecting, false, true);
  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  await card.getByRole("button", { name: "Copy code" }).click();
  await expect.poll(() => page.evaluate(() => window.__copyRequests)).toBe(1);
  await card.getByRole("button", { name: "Cancel", exact: true }).click();
  const add = card.getByRole("button", { name: "Add GitHub account" });
  await add.focus();
  await page.evaluate(() => window.__clipboardGate.resolve());
  await expect
    .poll(() => page.evaluate(() => window.__copiedCodes))
    .toEqual(["ABCD-EFGH"]);
  await expect(add).toBeFocused();
  await expect(card).not.toContainText("Code copied");
  await expect(card).not.toContainText("ABCD-EFGH");
});
