import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test, expect } from "./fixtures.mjs";
import { section } from "./navigation.mjs";
import { performInstaller } from "./control-installation-helpers.mjs";

async function openInstalled(page, store) {
  const installed = await store("control_install", {
    host: "copilot_cli",
    expected: null,
  });
  await page.goto("/?view=settings");
  await page.evaluate(() => window.__settingsIdle());
  await section(page, "Preferences");
  const controls = page.getByRole("group", {
    name: "Copilot CLI",
    exact: true,
  });
  await expect(
    controls.getByRole("button", { name: "Update", exact: true }),
  ).toBeEnabled();
  return { installed, controls };
}

async function observeControl(page, installError = null, statusFailures = 0) {
  await page.evaluate(
    ({ installError, statusFailures }) => {
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.__controlCalls = [];
      window.__TAURI_INTERNALS__.invoke = (command, args) => {
        if (command.startsWith("control_"))
          window.__controlCalls.push({ command, args });
        if (command === "control_install" && installError) {
          const code = installError;
          installError = null;
          return Promise.reject({ code });
        }
        if (command === "control_installation" && statusFailures-- > 0)
          return Promise.reject({ code: "installation_conflict" });
        return invoke(command, args);
      };
    },
    { installError, statusFailures },
  );
}

for (const mode of ["conflict", "installation_io"]) {
  test(`installer ${mode} refreshes native revision without replaying the mutation`, async ({
    page,
    store,
    dataRoot,
    ipc,
  }) => {
    const { installed, controls } = await openInstalled(page, store);
    let newer;
    if (mode === "conflict") {
      newer = await store("control_install", {
        host: "copilot_cli",
        expected: installed.revision,
        executable: join(dataRoot, "new-app"),
      });
    } else {
      const file = join(dataRoot, "skill-host", ".copilot", "mcp-config.json");
      const config = JSON.parse(await readFile(file, "utf8"));
      delete config.mcpServers["pr-sniper"];
      await writeFile(file, JSON.stringify(config));
      newer = await store("control_installation");
    }
    expect(newer.revision).not.toBe(installed.revision);
    await observeControl(page, mode === "installation_io" ? mode : null);
    const hold = ipc.holdNext(
      mode === "installation_io" ? "control_installation" : "control_install",
    );
    await controls.getByRole("button", { name: "Update", exact: true }).focus();
    await controls.getByRole("button", { name: "Update", exact: true }).click();
    await hold.arrived;
    await expect(
      controls.getByRole("button", { name: "Update", exact: true }),
    ).toBeDisabled();
    if (mode === "installation_io") {
      await page.evaluate(() => {
        const button = document.createElement("button");
        button.id = "other-focus";
        button.textContent = "Other control";
        document.body.append(button);
      });
      await page.locator("#other-focus").click();
      await page.locator("#other-focus").focus();
    }
    hold.release();
    await page.evaluate(() => window.__settingsIdle());
    const action = controls.getByRole("button", {
      // Either way the owned entry no longer matches this app: Repair.
      name: "Repair",
      exact: true,
    });
    await expect(action).toBeEnabled();
    await expect(controls.getByRole("alert")).toHaveText(
      mode === "conflict" ? "Changed" : "Unavailable",
    );
    if (mode === "installation_io")
      await expect(page.locator("#other-focus")).toBeFocused();
    else await expect(action).toBeFocused();
    const calls = await page.evaluate(() => window.__controlCalls);
    expect(
      calls.filter((call) => call.command === "control_install"),
    ).toHaveLength(1);
    expect(
      calls.filter((call) => call.command === "control_installation"),
    ).toHaveLength(1);
    await performInstaller(page, ipc, action, "control_install");
    await expect(
      controls.getByRole("button", { name: "Update", exact: true }),
    ).toBeEnabled();
    await expect(controls.getByRole("alert")).toBeHidden();
    const mutations = await page.evaluate(() =>
      window.__controlCalls.filter(
        (call) => call.command === "control_install",
      ),
    );
    expect(mutations).toHaveLength(2);
    expect(mutations[1].args.expected).toBe(newer.revision);
    expect((await store("control_installation")).state).toBe("installed");
  });
}

test("repeated status failures expose explicit read-only Retry and retain original failure", async ({
  page,
  store,
  dataRoot,
  ipc,
}) => {
  const { installed, controls } = await openInstalled(page, store);
  const newer = await store("control_install", {
    host: "copilot_cli",
    expected: installed.revision,
    executable: join(dataRoot, "new-app"),
  });
  await observeControl(page, null, 2);
  await controls.getByRole("button", { name: "Update", exact: true }).focus();
  await performInstaller(
    page,
    ipc,
    controls.getByRole("button", { name: "Update", exact: true }),
    "control_install",
  );
  const retry = controls.getByRole("button", { name: "Retry", exact: true });
  await expect(retry).toBeEnabled();
  await expect(retry).toBeFocused();
  await expect(controls.getByLabel("MCP status")).toHaveText("Unavailable");
  await expect(controls.getByRole("alert")).toHaveText("Changed");
  await retry.click();
  await page.evaluate(() => window.__settingsIdle());
  await expect(retry).toBeEnabled();
  await expect(controls.getByRole("alert")).toHaveText("Changed");
  expect(
    await page.evaluate(
      () =>
        window.__controlCalls.filter(
          (call) => call.command === "control_installation",
        ).length,
    ),
  ).toBe(2);
  await retry.click();
  await page.evaluate(() => window.__settingsIdle());
  const repair = controls.getByRole("button", { name: "Repair", exact: true });
  await expect(repair).toBeEnabled();
  await expect(repair).toBeFocused();
  await expect(controls.getByRole("alert")).toHaveText("Changed");
  expect(
    await page.evaluate(
      () =>
        window.__controlCalls.filter(
          (call) => call.command === "control_install",
        ).length,
    ),
  ).toBe(1);
  expect((await store("control_installation")).revision).toBe(newer.revision);
});
