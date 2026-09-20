import { test, expect } from "./fixtures.mjs";

test("an empty review queue exposes an immediate check", async ({
  page,
  store,
}) => {
  await store("save_repository", { repository: "example/project" });

  await page.goto("/?view=queue");
  await expect(
    page.getByRole("heading", { name: "Review Queue", exact: true }),
  ).toBeVisible();

  await expect(
    page.getByRole("button", { name: "Check Now", exact: true }),
  ).toBeEnabled();
});
