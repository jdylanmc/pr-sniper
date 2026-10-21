import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";

test("Copilot CLI installation uses native package writes and preserves edits on removal", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("snapshot");
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  const controls = page.getByRole("group", {
    name: "Copilot CLI",
    exact: true,
  });
  await expect(controls.getByLabel("Skill status")).toHaveText("Not installed");
  await controls.getByRole("button", { name: "Install", exact: true }).click();
  await expect(
    controls.getByRole("button", { name: "Update", exact: true }),
  ).toBeEnabled();
  const skill = join(
    dataRoot,
    "skill-host",
    ".copilot",
    "skills",
    "pr-sniper",
    "SKILL.md",
  );
  expect(await readFile(skill, "utf8")).toContain("name: pr-sniper");
  const config = JSON.parse(
    await readFile(
      join(dataRoot, "skill-host", ".copilot", "mcp-config.json"),
      "utf8",
    ),
  );
  expect(config.mcpServers["pr-sniper"].type).toBe("local");
  expect(config.mcpServers["pr-sniper"].args[0]).toMatch(
    /scripts[/\\]mcp-server\.mjs$/,
  );
  await controls.getByRole("button", { name: "Update", exact: true }).click();
  await page.evaluate(() => window.__settingsIdle());
  await writeFile(skill, "Human customization");
  await controls
    .getByRole("button", { name: "Remove PR Sniper skill" })
    .click();
  await expect(controls.getByRole("alert")).toHaveText("Conflict");
  const retry = controls.getByRole("button", { name: "Retry", exact: true });
  await expect(retry).toBeEnabled();
  await retry.click();
  await expect(retry).toBeEnabled();
  await expect(controls.getByRole("alert")).toHaveText("Conflict");
  expect(await readFile(skill, "utf8")).toBe("Human customization");
});

test("Copilot CLI removal and reload reflect actual isolated installation state", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("snapshot");
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  const controls = page.getByRole("group", {
    name: "Copilot CLI",
    exact: true,
  });
  await controls.getByRole("button", { name: "Install", exact: true }).click();
  await expect(
    controls.getByRole("button", { name: "Remove PR Sniper skill" }),
  ).toBeEnabled();
  await controls
    .getByRole("button", { name: "Remove PR Sniper skill" })
    .click();
  await expect(controls.getByLabel("Skill status")).toHaveText("Not installed");
  await expect(
    controls.getByRole("button", { name: "Remove PR Sniper skill" }),
  ).toBeDisabled();
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await expect(page.getByLabel("Skill status")).toHaveText("Not installed");
  const config = JSON.parse(
    await readFile(
      join(dataRoot, "skill-host", ".copilot", "mcp-config.json"),
      "utf8",
    ),
  );
  expect(config.mcpServers["pr-sniper"]).toBeUndefined();
  await expect(
    readFile(
      join(
        dataRoot,
        "skill-host",
        ".copilot",
        "skills",
        "pr-sniper",
        "SKILL.md",
      ),
    ),
  ).rejects.toThrow();
});

test("missing owned MCP entry offers explicit repair without losing other servers", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("snapshot");
  const installed = await store("control_install", {
    host: "copilot_cli",
    expected: null,
  });
  expect(installed.mcp_registered).toBe(true);
  const file = join(dataRoot, "skill-host", ".copilot", "mcp-config.json");
  await writeFile(
    file,
    JSON.stringify({ mcpServers: { other: { command: "preserved" } } }),
  );
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await expect(page.getByLabel("Skill status")).toHaveText("Incomplete");
  await page.evaluate(() => window.__settingsIdle());
  await page.getByRole("button", { name: "Repair", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Update", exact: true }),
  ).toBeEnabled();
  const config = JSON.parse(await readFile(file, "utf8"));
  expect(config.mcpServers.other).toEqual({ command: "preserved" });
  expect(config.mcpServers["pr-sniper"].type).toBe("local");
});
