import { expect, test } from "./fixtures.mjs";

test("verifying GitHub preserves drafts and does not enable automation", async ({
  page,
  store,
}) => {
  await store("save_repository", { repository: "jdylanmc/pr-sniper" });
  const before = (await store("snapshot")).settings;
  const calls = [];
  await page.exposeFunction("__githubRead", (command, args) => {
    calls.push({ command, args });
    return {
      identity: { id: "6954990", login: "jdylanmc" },
      repository: { id: "1376547672", name: "jdylanmc/pr-sniper" },
      capabilities: { read: true, comment: "available" },
    };
  });
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) =>
      command === "verify_github_connection"
        ? window.__githubRead(command, args)
        : original(command, args);
  });
  await page.goto("/?view=settings");
  const prompt = page
    .getByRole("form", { name: "Global defaults", exact: true })
    .getByLabel("Review prompt", { exact: true });
  await prompt.fill("Keep this unsaved review prompt.");
  const card = page.getByRole("article", {
    name: "jdylanmc/pr-sniper",
    exact: true,
  });
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(card.getByRole("status")).toContainText("jdylanmc (6954990)");
  await expect(card.getByRole("status")).toContainText(
    "not publication authorization",
  );
  await expect(prompt).toHaveValue("Keep this unsaved review prompt.");
  expect(calls).toEqual([
    {
      command: "verify_github_connection",
      args: { id: before.repositories[0].id, expectedAccountId: null },
    },
  ]);
  expect((await store("snapshot")).settings).toEqual(before);
});
