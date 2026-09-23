import { expect, test } from "./fixtures.mjs";

test("GitHub App connection states are honest and contain no credentials", async ({
  page,
}) => {
  let state = { state: "disconnected" };
  await page.exposeFunction("__githubAuth", (command) => {
    if (command === "begin_github_auth")
      state = {
        state: "connecting",
        user_code: "ABCD-EFGH",
        verification_uri: "https://github.com/login/device",
        expires_in_seconds: 900,
        interval_seconds: 5,
      };
    if (command === "poll_github_auth")
      state = {
        state: "connected",
        account_id: "6954990",
        login: "jdylanmc",
      };
    if (command === "cancel_github_auth") state = { state: "disconnected" };
    return state;
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      [
        "github_auth_state",
        "begin_github_auth",
        "poll_github_auth",
        "cancel_github_auth",
      ].includes(command)
        ? window.__githubAuth(command, args)
        : original(command, args);
  });
  await page.goto("/?view=settings");

  const card = page.locator(".github-auth-card");
  await expect(card.getByRole("status")).toContainText("Disconnected");
  await card.getByRole("button", { name: "Connect GitHub" }).click();
  await expect(card.getByRole("status")).toContainText("ABCD-EFGH");
  await expect(card.getByRole("status")).toContainText("at least 5 seconds");
  await card.getByRole("button", { name: "Check authorization" }).click();
  await expect(card.getByRole("status")).toContainText(
    "Connected as jdylanmc (6954990)",
  );
  await expect(card.getByRole("status")).toContainText(
    "repository access is not implied",
  );
  await expect(page.locator("body")).not.toContainText("access_token");
  await expect(page.locator("body")).not.toContainText("refresh_token");
});

test("reconnect-required state never reports connected", async ({ page }) => {
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "github_auth_state"
        ? Promise.resolve({ state: "reconnect_required" })
        : original(command, args);
  });
  await page.goto("/?view=settings");
  const card = page.locator(".github-auth-card");
  await expect(card.getByRole("status")).toContainText("Reconnect required");
  await expect(card.getByRole("status")).not.toContainText("Connected as");
});
