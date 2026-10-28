import { expect, test } from "./fixtures.mjs";
import { queueFixture, showQueueGroup } from "./queue-fixture.mjs";

test.use({ viewport: { width: 408, height: 744 } });

const tab = (page, name) =>
  page
    .getByRole("navigation", { name: "Application destinations" })
    .getByRole("button", { name, exact: true });

async function park(store, failure, second) {
  const fixture = await queueFixture(store);
  const publication = fixture.state.publications.find(
    (entry) => entry.review.job.number === 3,
  );
  publication.operation.failure = failure;
  if (second) {
    const other = fixture.state.publications.find(
      (entry) => entry.review.job.number === 2,
    );
    other.operation.state = "manual_retry";
    other.operation.failure = second;
  }
  await store("seed_queue_state", fixture.state);
}

test("a publication's credentials park routes Fix to its GitHub account and offers Retry", async ({
  page,
  store,
}) => {
  await park(store, "credentials");
  await page.goto("/");
  await showQueueGroup(page, "attention");
  const failed = page.getByRole("article", {
    name: "example/repo #3",
    exact: true,
  });
  await expect(failed).toHaveAttribute("data-state", "failed");
  await expect(failed).toContainText("Credentials");
  await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
  await expect(failed.locator("[data-fix-account]")).toHaveText("Fix");
  await expect(page.locator("[data-queue-attention]")).toHaveText("2");
  await failed.locator("[data-fix-account]").click();
  await expect(
    page.getByRole("heading", { name: "GitHub", exact: true }),
  ).toBeVisible();
});

test("a review's credentials park routes Fix to its Copilot account", async ({
  page,
  store,
}) => {
  const fixture = await queueFixture(store);
  const review = fixture.state.reviews.find((entry) => entry.job.number === 3);
  review.operation.state = "failed";
  review.operation.failure = "credentials";
  review.result = null;
  fixture.state.publications = fixture.state.publications.filter(
    (entry) => entry.review.job.number !== 3,
  );
  await store("seed_queue_state", fixture.state);
  await page.goto("/");
  await showQueueGroup(page, "attention");
  const failed = page.getByRole("article", {
    name: "example/repo #3",
    exact: true,
  });
  await expect(failed).toContainText("Credentials");
  await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
  await failed.locator("[data-fix-account]").click();
  await expect(
    page.getByRole("heading", { name: "GitHub Copilot", exact: true }),
  ).toBeVisible();
});

test("every parked failure is one Needs attention row with a cause chip, Retry and never a Running row", async ({
  page,
  store,
}) => {
  for (const failure of ["network", "permission", "timeout", "permanent"]) {
    await park(store, failure);
    await store("panel_navigate", { route: { tab: "queue" } });
    await page.goto("/");
    await showQueueGroup(page, "attention");
    const failed = page.getByRole("article", {
      name: "example/repo #3",
      exact: true,
    });
    await expect(failed).toHaveCount(1);
    await expect(failed).toHaveAttribute("data-state", "failed");
    await expect(failed.locator(".queue-state")).toHaveText(
      {
        network: "Network",
        permission: "Permission",
        timeout: "Network",
        permanent: "Unverified",
      }[failure],
    );
    await expect(failed.locator("[data-fix-account]")).toHaveCount(0);
    await expect(failed.getByRole("button", { name: "Retry" })).toHaveCount(1);
    await expect(page.locator("[data-queue-attention]")).toHaveText("2");
    await tab(page, "Running").click();
    await expect(page.locator('[data-work-group="failures"]')).toHaveCount(0);
    await expect(page.locator("[data-running-list] article")).toHaveCount(0);
    await expect(page.locator(".work-retry-all")).toHaveCount(0);
  }
});

test("Retry all heads Needs attention and counts every parked row, credentials included", async ({
  page,
  store,
}) => {
  await park(store, "network", "permission");
  await page.goto("/");
  await showQueueGroup(page, "attention");
  const all = page.getByRole("button", { name: "Retry all failed" });
  await expect(all).toHaveText("2");
  await expect(
    page
      .locator('[data-queue-group="attention"]')
      .first()
      .locator("> *")
      .first(),
  ).toHaveClass(/work-retry-all/);
  await park(store, "credentials", "permission");
  await page.goto("/");
  await showQueueGroup(page, "attention");
  await expect(all).toHaveText("2");
  await expect(
    page.getByRole("article", { name: "example/repo #3", exact: true }),
  ).toContainText("Credentials");
});
