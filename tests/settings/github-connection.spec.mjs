import { expect, test } from "./fixtures.mjs";
import {
  closeDialog,
  repositorySettings,
  saveChanges,
  section,
} from "./navigation.mjs";

async function connection(page, name = "jdylanmc/pr-sniper") {
  const modal = await repositorySettings(page, name);
  await modal.getByText("Repository and connection", { exact: true }).click();
  return modal;
}

const verified = {
  identity: { id: "6954990", login: "jdylanmc" },
  repository: { id: "1376547672", name: "jdylanmc/pr-sniper" },
  capabilities: { read: true, comment: "available" },
};

async function githubFixture(page, store, handler) {
  await store("save_repository", { repository: "jdylanmc/pr-sniper" });
  await page.exposeFunction("__githubResponse", handler);
  await page.addInitScript(() => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (
        !["verify_github_connection", "read_github_metadata"].includes(command)
      )
        return original(command, args);
      const response = await window.__githubResponse(command, args);
      if ("error" in response) throw response.error;
      return response.ok;
    };
  });
  await page.goto("/?view=settings");
  return connection(page);
}

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
  await section(page, "Review defaults");
  const prompt = page.getByLabel("Review prompt", { exact: true });
  await prompt.fill("Keep this unsaved review prompt.");
  const card = await connection(page);
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(card.getByRole("status")).toContainText("jdylanmc (6954990)");
  await expect(card.getByRole("status")).toContainText(
    "not publication authorization",
  );
  await closeDialog(page);
  await section(page, "Review defaults");
  await expect(prompt).toHaveValue("Keep this unsaved review prompt.");
  expect(calls).toEqual([
    {
      command: "verify_github_connection",
      args: { id: before.repositories[0].id },
    },
  ]);
  expect((await store("snapshot")).settings).toEqual(before);
});

for (const [error, expected] of [
  ["missing_cli", "development GitHub CLI probe is unavailable"],
  ["broken_cli", "development GitHub CLI probe is broken"],
  ["signed_out", "App authorization is missing, expired, or rejected"],
  ["wrong_identity", "does not match"],
  ["missing_read_permission", "denied repository or pull-request read"],
  ["rate_limited", "rate limited"],
  ["network", "Could not reach GitHub securely"],
  ["provider_failure", "could not complete this read"],
  ["incomplete_read", "No partial result was accepted"],
  ["gho_untrusted_secret_error_body", "No raw error details"],
]) {
  test(`connection reports ${error} without enabling metadata or leaking errors`, async ({
    page,
    store,
  }) => {
    const card = await githubFixture(page, store, () => ({ error }));
    await card
      .getByRole("button", { name: "Verify GitHub connection", exact: true })
      .click();
    await expect(card.getByRole("status")).toContainText(expected);
    await expect(
      card.getByRole("button", { name: "Read PR metadata", exact: true }),
    ).toBeDisabled();
    await expect(page.locator("body")).not.toContainText(
      "gho_untrusted_secret",
    );
  });
}

for (const [comment, message] of [
  ["unknown", "Comment permission unverified"],
  ["unavailable", "Comment permission unavailable"],
]) {
  test(`read access does not fabricate ${comment} comment permission`, async ({
    page,
    store,
  }) => {
    const card = await githubFixture(page, store, () => ({
      ok: { ...verified, capabilities: { read: true, comment } },
    }));
    await card
      .getByRole("button", { name: "Verify GitHub connection", exact: true })
      .click();
    await expect(card.getByRole("status")).toContainText(message);
    await expect(card.getByRole("status")).toContainText(
      "not publication authorization",
    );
  });
}

test("metadata pins verified identities, renders all files safely and clears failed results", async ({
  page,
  store,
}) => {
  let reads = 0;
  const calls = [];
  const card = await githubFixture(page, store, (command, args) => {
    calls.push({ command, args });
    if (command === "verify_github_connection") return { ok: verified };
    if (++reads > 1) return { error: "revision_changed" };
    return {
      ok: {
        connection: verified,
        pull_requests: [
          {
            number: 31,
            title: "<script>bad()</script>",
            state: "open",
            draft: false,
            head_sha: "a".repeat(40),
            author: { id: "42", login: "author" },
            requested_reviewers: [{ id: "6954990", login: "jdylanmc" }],
            files: [
              { path: "first.rs", status: "added" },
              { path: "last.rs", status: "renamed" },
            ],
          },
        ],
      },
    };
  });
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await card
    .getByRole("button", { name: "Read PR metadata", exact: true })
    .click();
  await expect(card.getByRole("status")).toContainText(
    "1 PRs, 2 changed files",
  );
  await card.locator(".connection summary").click();
  await expect(card.locator("li")).toHaveText([
    "added: first.rs",
    "renamed: last.rs",
  ]);
  await expect(card.locator("script")).toHaveCount(0);
  expect(calls[1].args.expectedAccountId).toBe("6954990");
  expect(calls[1].args.expectedRepositoryId).toBe("1376547672");
  await card
    .getByRole("button", { name: "Read PR metadata", exact: true })
    .click();
  await expect(card.getByRole("status")).toContainText(
    "changed during the read",
  );
  await expect(card.locator(".connection summary")).toHaveCount(0);
  await expect(
    card.getByRole("button", { name: "Read PR metadata", exact: true }),
  ).toBeDisabled();
});

test("retargeting a local repository invalidates the old connection", async ({
  page,
  store,
}) => {
  const card = await githubFixture(page, store, () => ({ ok: verified }));
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(card.getByRole("status")).toContainText("jdylanmc (6954990)");
  await card
    .getByRole("button", { name: "Edit repository", exact: true })
    .click();
  const editor = page.getByRole("dialog", {
    name: "Edit repository",
    exact: true,
  });
  await editor
    .getByLabel("GitHub repository", { exact: true })
    .fill("other/target");
  await editor
    .getByRole("button", { name: "Use repository", exact: true })
    .click();
  await saveChanges(page);
  const target = await connection(page, "other/target");
  await expect(target.getByRole("status")).toContainText("Not verified");
  await expect(
    target.getByRole("button", { name: "Read PR metadata", exact: true }),
  ).toBeDisabled();
});

test("metadata replaces stale capability evidence for the same account and repository", async ({
  page,
  store,
}) => {
  const capabilities = ["unavailable", "unknown", "available"];
  let reads = 0;
  const card = await githubFixture(page, store, (command) => {
    if (command === "verify_github_connection") return { ok: verified };
    return {
      ok: {
        connection: {
          ...verified,
          capabilities: { read: true, comment: capabilities[reads++] },
        },
        pull_requests: [],
      },
    };
  });
  await card
    .getByRole("button", { name: "Verify GitHub connection", exact: true })
    .click();
  await expect(card.getByRole("status")).toContainText(
    "Comment scope available",
  );
  for (const message of [
    "Comment permission unavailable",
    "Comment permission unverified",
    "Comment scope available",
  ]) {
    await card
      .getByRole("button", { name: "Read PR metadata", exact: true })
      .click();
    await expect(card.getByRole("status")).toContainText(message);
    await expect(card.getByRole("status")).toContainText(
      "Complete metadata: 0 PRs",
    );
  }
});
