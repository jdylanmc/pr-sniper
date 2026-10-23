import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { performInstaller } from "./control-installation-helpers.mjs";

test("Copilot CLI setup registers only the app-bundled MCP server and preserves a customized entry", async ({
  page,
  store,
  dataRoot,
  ipc,
}) => {
  await store("snapshot");
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  const controls = page.getByRole("group", {
    name: "Copilot CLI",
    exact: true,
  });
  await expect(controls.getByLabel("MCP status")).toHaveText("Not installed");
  await expect(controls.getByLabel("Skill status")).toHaveText("Not detected");
  await expect(
    controls.getByLabel("Skill install command", { exact: true }),
  ).toHaveText(
    "npx skills add jdylanmc/pr-sniper --skill pr-sniper -g -a github-copilot",
  );
  await expect(
    controls.getByRole("button", { name: "Copy skill install command" }),
  ).toBeEnabled();
  await performInstaller(
    page,
    ipc,
    controls.getByRole("button", { name: "Install", exact: true }),
    "control_install",
  );
  await expect(controls.getByLabel("MCP status")).toHaveText("Installed");
  await expect(
    controls.getByRole("button", { name: "Update", exact: true }),
  ).toBeEnabled();
  const file = join(dataRoot, "skill-host", ".copilot", "mcp-config.json");
  const config = JSON.parse(await readFile(file, "utf8"));
  const entry = config.mcpServers["pr-sniper"];
  expect(entry.type).toBe("local");
  expect(entry.args[0]).toMatch(/mcp-server\.mjs$/);
  expect(entry.args[1]).toBe("--app");
  // The app never writes a skill; the skills CLI owns it.
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
  await performInstaller(
    page,
    ipc,
    controls.getByRole("button", { name: "Update", exact: true }),
    "control_install",
  );
  config.mcpServers["pr-sniper"] = { command: "Human customization" };
  await writeFile(file, JSON.stringify(config));
  await performInstaller(
    page,
    ipc,
    controls.getByRole("button", { name: "Remove PR Sniper MCP server" }),
    "control_uninstall",
  );
  await expect(controls.getByRole("alert")).toHaveText("Conflict");
  const retry = controls.getByRole("button", { name: "Retry", exact: true });
  await expect(retry).toBeEnabled();
  await retry.click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(retry).toBeEnabled();
  await expect(controls.getByRole("alert")).toHaveText("Conflict");
  expect(
    JSON.parse(await readFile(file, "utf8")).mcpServers["pr-sniper"],
  ).toEqual({ command: "Human customization" });
});

test("an installed pr-sniper skill is detected read-only", async ({
  page,
  store,
  dataRoot,
}) => {
  await store("snapshot");
  const skill = join(dataRoot, "skill-host", ".agents", "skills", "pr-sniper");
  await mkdir(skill, { recursive: true });
  const body = "---\nname: pr-sniper\ndescription: fixture\n---\n";
  await writeFile(join(skill, "SKILL.md"), body);
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  const controls = page.getByRole("group", {
    name: "Copilot CLI",
    exact: true,
  });
  await expect(controls.getByLabel("Skill status")).toHaveText("Detected");
  await expect(controls.getByLabel("MCP status")).toHaveText("Not installed");
  expect(await readFile(join(skill, "SKILL.md"), "utf8")).toBe(body);
});

test("Copilot CLI removal and reload reflect actual isolated installation state", async ({
  page,
  store,
  dataRoot,
  ipc,
}) => {
  await store("snapshot");
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  const controls = page.getByRole("group", {
    name: "Copilot CLI",
    exact: true,
  });
  await performInstaller(
    page,
    ipc,
    controls.getByRole("button", { name: "Install", exact: true }),
    "control_install",
  );
  await expect(
    controls.getByRole("button", { name: "Remove PR Sniper MCP server" }),
  ).toBeEnabled();
  await performInstaller(
    page,
    ipc,
    controls.getByRole("button", { name: "Remove PR Sniper MCP server" }),
    "control_uninstall",
  );
  await expect(controls.getByLabel("MCP status")).toHaveText("Not installed");
  await expect(
    controls.getByRole("button", { name: "Remove PR Sniper MCP server" }),
  ).toBeDisabled();
  await page.reload();
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  await expect(page.getByLabel("MCP status")).toHaveText("Not installed");
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
  ipc,
}, testInfo) => {
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
  const started = Date.now();
  const diagnostics = [];
  const record = (event) => {
    if (diagnostics.length >= 16) return;
    const entry = { ...event, elapsed_ms: Date.now() - started };
    diagnostics.push(entry);
    console.info(`[control-installer] ${JSON.stringify(entry)}`);
  };
  await page.exposeFunction("__installerTiming", record);
  await page.evaluate(() => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = async (command, args) => {
      if (!command.startsWith("control_")) return invoke(command, args);
      await window.__installerTiming({ command, phase: "requested" });
      try {
        const result = await invoke(command, args);
        await window.__installerTiming({
          command,
          phase: "response",
          state: result.state,
          registered: result.mcp_registered,
        });
        return result;
      } catch (error) {
        const code = typeof error === "string" ? error : error?.code;
        await window.__installerTiming({
          command,
          phase: "error",
          code: /^[a-z_]{1,64}$/.test(code) ? code : "native_error",
        });
        throw error;
      }
    };
  });
  await section(page, "Preferences");
  await expect(page.getByLabel("MCP status")).toHaveText("Incomplete");
  await page.evaluate(() => window.__settingsIdle());
  try {
    await performInstaller(
      page,
      ipc,
      page.getByRole("button", { name: "Repair", exact: true }),
      "control_install",
      {
        delay: 6500,
        record,
        afterNative: async () => {
          const committed = JSON.parse(await readFile(file, "utf8"));
          expect(committed.mcpServers["pr-sniper"].type).toBe("local");
          record({ phase: "native_committed", registered: true });
        },
      },
    );
    await expect(
      page.getByRole("button", { name: "Update", exact: true }),
    ).toBeEnabled();
    const config = JSON.parse(await readFile(file, "utf8"));
    expect(config.mcpServers.other).toEqual({ command: "preserved" });
    expect(config.mcpServers["pr-sniper"].type).toBe("local");
  } finally {
    await testInfo.attach("installer-commands", {
      body: JSON.stringify(diagnostics),
      contentType: "application/json",
    });
  }
});
