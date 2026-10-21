import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import net from "node:net";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";

const repository = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);
const identifier = "com.jdylanmc.pr-sniper.tests.control-ci";
const options = parseArgs({
  options: { executable: { type: "string" } },
  strict: true,
}).values;
assert(options.executable, "--executable must identify a test-built app");
assert(
  ["darwin", "win32"].includes(process.platform),
  "Native acceptance requires macOS or Windows",
);
if (process.platform === "win32") {
  assert(
    process.env.GITHUB_ACTIONS === "true" &&
      process.env.RUNNER_ENVIRONMENT === "github-hosted",
    "The production-identifier Windows executable runs only on a fresh hosted runner",
  );
}
const target = await fs.realpath(path.join(repository, "src-tauri", "target"));
const source = await fs.realpath(path.resolve(repository, options.executable));
const sourceRelative = path.relative(target, source);
assert(
  !path.isAbsolute(sourceRelative) && !sourceRelative.startsWith(".."),
  "Executable must be a build artifact under this checkout's target",
);
const output = path.join(target, "control-native-acceptance");
await fs.mkdir(output, { recursive: true });
const root = await fs.mkdtemp(path.join(output, "run-"));
const checkout = spawnSync("git", ["rev-parse", "HEAD"], {
  cwd: repository,
  encoding: "utf8",
  timeout: 10_000,
});
assert.equal(
  checkout.status,
  0,
  "Native acceptance requires source provenance",
);
const sourceState = spawnSync("git", ["status", "--porcelain"], {
  cwd: repository,
  encoding: "utf8",
  timeout: 10_000,
});
assert.equal(sourceState.status, 0, "Native acceptance requires source state");
const receipt = {
  status: "RUNNING",
  platform: process.platform,
  source,
  source_sha256: createHash("sha256")
    .update(await fs.readFile(source))
    .digest("hex"),
  commit: checkout.stdout.trim(),
  source_dirty: sourceState.stdout.length !== 0,
  checks: [],
  operations: [],
  owned_processes: [],
  native_transport: [],
};
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const run = (file, args, options = {}) => {
  const result = spawnSync(file, args, {
    encoding: "utf8",
    timeout: 15_000,
    ...options,
  });
  if (result.error) throw result.error;
  assert.equal(
    result.status,
    0,
    `${path.basename(file)} failed (${result.status}): ${result.stderr}`,
  );
  return result.stdout;
};
let executable;
let cli;
let env;
let mcp;
let installation;
function setup(command, args = {}) {
  const bridge = path.join(
    target,
    "debug/examples",
    process.platform === "win32" ? "settings_bridge.exe" : "settings_bridge",
  );
  const response = JSON.parse(
    run(bridge, [path.join(root, "installer-store")], {
      input: JSON.stringify({ command, args }),
      timeout: 30000,
    }),
  );
  assert(response.ok, `Native setup failed: ${JSON.stringify(response.error)}`);
  return response.ok;
}
const owned = new Map();

function observedProcesses() {
  if (!executable) return [];
  if (process.platform === "win32") {
    const script =
      "$ErrorActionPreference='Stop'; @(" +
      "Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $env:PR_SNIPER_ACCEPTANCE_EXECUTABLE } " +
      "| ForEach-Object { [pscustomobject]@{ pid=[int]$_.ProcessId; born=$_.CreationDate.ToUniversalTime().ToString('o') } }) | ConvertTo-Json -Compress";
    const json = run(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", script],
      { env: { ...process.env, PR_SNIPER_ACCEPTANCE_EXECUTABLE: executable } },
    ).trim();
    const result = json ? JSON.parse(json) : [];
    return Array.isArray(result) ? result : [result];
  }
  return run("ps", ["-axo", "pid=,lstart=,command="], {
    env: { ...process.env, LC_ALL: "C" },
  })
    .split("\n")
    .map((line) => line.match(/^\s*(\d+)\s+(.{24})\s+(.+)$/))
    .filter((match) => match && match[3] === `${executable} --background`)
    .map((match) => ({ pid: Number(match[1]), born: match[2] }));
}

function claimProcesses() {
  // This exact copied executable exists only inside this run's freshly-created
  // directory. Never claim another installed app or a process by name.
  for (const process of observedProcesses()) {
    const existing = owned.get(process.pid);
    assert(
      !existing || existing.born === process.born,
      "Owned PID identity changed",
    );
    if (!existing) {
      owned.set(process.pid, process);
      receipt.owned_processes.push(process);
    }
  }
}

async function stopOwned() {
  claimProcesses();
  for (const process of observedProcesses()) {
    assert.equal(
      owned.get(process.pid)?.born,
      process.born,
      "Refusing to stop an unowned process",
    );
    globalThis.process.kill(process.pid, "SIGTERM");
  }
  const deadline = Date.now() + 15_000;
  while (observedProcesses().length && Date.now() < deadline) await pause(200);
  assert.equal(
    observedProcesses().length,
    0,
    "Owned native app failed to terminate",
  );
}

function call(input) {
  const started = Date.now();
  const result = spawnSync(process.execPath, [cli], {
    cwd: root,
    env,
    input: typeof input === "string" ? input : JSON.stringify(input),
    encoding: "utf8",
    timeout: 140_000,
    maxBuffer: 2 * 1024 * 1024,
  });
  claimProcesses();
  if (result.error) throw result.error;
  const response = JSON.parse(result.stdout);
  receipt.operations.push({
    operation: typeof input === "string" ? "malformed_json" : input.operation,
    exit_code: result.status,
    ok: response.ok,
    code: response.error?.code ?? null,
    elapsed_ms: Date.now() - started,
    native_alive: observedProcesses().length,
  });
  assert.equal(
    result.status,
    response.ok ? 0 : typeof input === "string" ? 2 : 1,
  );
  return response;
}

function okay(input) {
  const response = call(input);
  assert.equal(
    response.ok,
    true,
    `Native operation ${input.operation} failed: ${response.error?.code}`,
  );
  return response.result;
}

async function fragmentedRejection() {
  const endpoint = JSON.parse(
    await fs.readFile(path.join(root, "data/control/endpoint.json"), "utf8"),
  );
  const payload = Buffer.from(
    JSON.stringify({
      protocol: 1,
      token: endpoint.token,
      request: {
        operation: "login",
        request_id: randomUUID(),
        issued_at: Math.floor(Date.now() / 1000),
      },
    }),
  );
  const response = await new Promise((resolve, reject) => {
    const socket = net.createConnection({
      host: "127.0.0.1",
      port: endpoint.port,
    });
    let received = Buffer.alloc(0);
    const deadline = setTimeout(() => {
      socket.destroy();
      reject(new Error("Fragmented native request timed out"));
    }, 4000);
    let write;
    const done = (error, value) => {
      clearTimeout(deadline);
      clearTimeout(write);
      socket.destroy();
      if (error) reject(error);
      else resolve(value);
    };
    socket.on("connect", () => {
      const header = Buffer.alloc(12);
      Buffer.from("SNIPER1\n").copy(header);
      header.writeUInt32BE(payload.length, 8);
      socket.write(header);
      write = setTimeout(() => socket.write(payload), 100);
    });
    socket.on("data", (bytes) => {
      received = Buffer.concat([received, bytes]);
      if (received.length > 1024 * 1024 + 4)
        return done(new Error("Oversized native reply"));
      if (
        received.length >= 4 &&
        received.length === received.readUInt32BE(0) + 4
      ) {
        try {
          done(null, JSON.parse(received.subarray(4)));
        } catch {
          done(new Error("Invalid native reply"));
        }
      }
    });
    socket.on("error", () =>
      done(new Error("Fragmented native request lost its connection")),
    );
    socket.on("end", () => {
      if (
        received.length < 4 ||
        received.length !== received.readUInt32BE(0) + 4
      )
        done(new Error("Fragmented native request lost its reply"));
    });
  });
  assert.equal(response.ok, false);
  assert.equal(response.error?.code, "invalid_request");
  receipt.checks.push(
    "actual native listener accepts a fragmented frame and returns typed rejection",
  );
}

try {
  if (process.platform === "darwin") {
    const bundle = path.resolve(source, "../../..");
    assert.equal(path.extname(bundle), ".app");
    const actual = run("plutil", [
      "-extract",
      "CFBundleIdentifier",
      "raw",
      "-o",
      "-",
      path.join(bundle, "Contents/Info.plist"),
    ]).trim();
    assert.equal(
      actual,
      identifier,
      "Build with tests/control/tauri.acceptance.conf.json; never launch the installed identifier locally",
    );
    run("codesign", ["--verify", "--strict", bundle]);
    const copied = path.join(root, "PR Sniper Control Acceptance.app");
    await fs.cp(bundle, copied, { recursive: true });
    executable = path.join(copied, "Contents", "MacOS", path.basename(source));
  } else {
    executable = path.join(root, "pr-sniper.exe");
    await fs.copyFile(source, executable);
  }
  installation = setup("control_install", {
    host: "copilot_cli",
    expected: null,
    executable,
  });
  assert.equal(installation.mcp_registered, true);
  const hostHome = path.join(root, "installer-store/skill-host");
  const skill = path.join(hostHome, ".copilot/skills/pr-sniper");
  const registered = JSON.parse(
    await fs.readFile(path.join(hostHome, ".copilot/mcp-config.json"), "utf8"),
  );
  assert.deepEqual(registered.mcpServers["pr-sniper"].args, [
    path.join(skill, "scripts/mcp-server.mjs"),
  ]);
  receipt.checks.push(
    "native installer creates verified embedded skill/server and owned stdio registration in synthetic home",
  );
  cli = path.join(skill, "scripts", "pr-sniper.mjs");
  env = {
    ...process.env,
    PR_SNIPER_DATA_DIR: path.join(root, "data"),
    PR_SNIPER_KEYCHAIN_SERVICE: `com.jdylanmc.pr-sniper.tests.control-${randomUUID()}`,
    PR_SNIPER_LIVE: "0",
  };
  assert.deepEqual(observedProcesses(), []);
  const initial = okay({ operation: "inspect" });
  assert.equal(
    observedProcesses().length,
    1,
    "CLI must launch exactly one native app",
  );
  assert.equal(initial.counts.repositories, 0);
  assert.equal(initial.counts.provider_accounts, 0);
  assert.equal(initial.counts.copilot_accounts, 0);
  assert.equal(initial.automation.paused, false);
  receipt.checks.push(
    "bundled CLI launched real app into fresh synthetic profile",
  );
  const id = randomUUID();
  const edit = {
    kind: "doctrine",
    id,
    expected: null,
    value: {
      id,
      title: "Native control acceptance",
      body: "Synthetic local-only fixture.",
    },
  };
  assert.equal(
    okay({ operation: "save", revision: initial.revision, edit }).state,
    "saved",
  );
  const stale = call({ operation: "save", revision: initial.revision, edit });
  assert.equal(stale.error?.code, "conflict");
  receipt.checks.push("native resource save and stale-save rejection");
  assert.equal(okay({ operation: "pause", paused: true }).paused, true);
  for (const forbidden of [
    { operation: "login" },
    { operation: "inspect", unexpected: true },
    "{",
  ]) {
    assert.equal(call(forbidden).error?.code, "invalid_request");
  }
  receipt.checks.push(
    "native pause and rejection of login/unknown fields/malformed JSON",
  );
  await fragmentedRejection();
  const before = observedProcesses()[0];
  await stopOwned();
  const restored = okay({ operation: "inspect" });
  assert.equal(observedProcesses().length, 1);
  assert.notDeepEqual(
    observedProcesses()[0],
    before,
    "CLI must actually relaunch the closed app",
  );
  assert.equal(restored.automation.paused, true);
  let found = false;
  let offset = 0;
  do {
    const page = okay({ operation: "resources", kind: "doctrines", offset });
    found ||= page.values.some(
      (entry) => entry.id === id && entry.body === edit.value.body,
    );
    offset = page.next_offset;
  } while (offset !== null);
  assert(found, "Native saved doctrine must survive process restart");
  receipt.checks.push(
    "real close/relaunch preserves pause and committed resource",
  );
  await stopOwned();
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [path.join(skill, "scripts/mcp-server.mjs")],
    env,
    stderr: "pipe",
  });
  let diagnostics = "";
  transport.stderr?.on("data", (bytes) => {
    diagnostics = (diagnostics + bytes).slice(-8192);
  });
  mcp = new Client({ name: "pr-sniper-native-acceptance", version: "1" });
  await mcp.connect(transport, { timeout: 10000 });
  const tools = await mcp.listTools();
  assert(tools.tools.some((tool) => tool.name === "save_doctrine"));
  assert(!tools.tools.some((tool) => tool.name === "login"));
  assert.equal(
    observedProcesses().length,
    0,
    "MCP discovery must not launch the app",
  );
  const tool = async (name, args = {}, expectedError) => {
    const response = await mcp.callTool({ name, arguments: args }, undefined, {
      timeout: 140000,
    });
    claimProcesses();
    assert.equal(
      Boolean(response.isError),
      Boolean(expectedError),
      `${name}: ${JSON.stringify(response)} ${diagnostics}`,
    );
    const native = response.structuredContent;
    assert(native, `${name} must return structured native data`);
    if (expectedError) assert.equal(native.error?.code, expectedError);
    else assert.equal(native.ok, true, JSON.stringify(native));
    return native.result;
  };
  const prepared = async () =>
    (await mcp.callTool({ name: "prepare_request", arguments: {} }))
      .structuredContent;
  const summary = await tool("inspect");
  assert.equal(summary.automation.paused, true);
  assert.equal(observedProcesses().length, 1);
  for (const name of [
    "list_agents",
    "list_doctrines",
    "list_voices",
    "list_repositories",
    "get_preferences",
    "list_repository_accounts",
    "list_copilot_accounts",
    "list_queue",
    "list_health",
    "get_notifications",
  ]) {
    await tool(name);
  }
  const mcpId = randomUUID();
  const doctrine = {
    id: mcpId,
    title: "MCP native acceptance",
    body: "SDK stdio to real native Store.",
  };
  const save = {
    ...(await prepared()),
    revision: summary.revision,
    id: mcpId,
    expected: null,
    value: doctrine,
  };
  await tool("save_doctrine", save);
  await tool("save_doctrine", { ...save, ...(await prepared()) }, "conflict");
  await tool("set_pause", { ...(await prepared()), paused: true });
  await tool(
    "list_models",
    { account_id: "not-connected" },
    "operation_failed",
  );
  const invalid = await mcp.callTool({ name: "save_doctrine", arguments: {} });
  assert.equal(invalid.isError, true, "Malformed tool input must be rejected");
  await assert.rejects(
    mcp.callTool({ name: "login", arguments: {} }),
    /Tool login not found/,
  );
  await stopOwned();
  assert.equal((await tool("inspect")).automation.paused, true);
  let mcpFound = false;
  let mcpOffset = 0;
  do {
    const page = await tool("list_doctrines", { offset: mcpOffset });
    mcpFound ||= page.values.some(
      (entry) => entry.id === mcpId && entry.body === doctrine.body,
    );
    mcpOffset = page.next_offset;
  } while (mcpOffset !== null);
  assert(mcpFound, "MCP save must persist through native app restart");
  await mcp.close();
  mcp = undefined;
  receipt.checks.push(
    "packaged SDK stdio discovers typed tools, launches native app, reads all views, saves/conflicts/rejects invalid and login operations, preserves pause and state after close/relaunch",
  );
  receipt.status = "PASS";
} catch (error) {
  receipt.status = "FAIL";
  receipt.error = String(error.message ?? error);
  process.exitCode = 1;
} finally {
  if (mcp) await mcp.close();
  receipt.native_alive_before_cleanup = observedProcesses().length;
  try {
    const file = path.join(root, "data/control/transport-errors.json");
    const stat = await fs.stat(file);
    assert(
      stat.size <= 16 * 1024,
      "Native transport diagnostics exceeded bound",
    );
    receipt.native_transport = JSON.parse(await fs.readFile(file, "utf8"));
  } catch (error) {
    if (error.code !== "ENOENT")
      receipt.native_transport_read_error = error.code ?? "unavailable";
  }
  try {
    await stopOwned();
    if (installation) {
      const removed = setup("control_uninstall", {
        host: "copilot_cli",
        expected: installation.revision,
      });
      assert.equal(removed.state, "absent");
      receipt.checks.push(
        "native integration uninstall removes owned package and server entry",
      );
    }
    receipt.cleanup =
      "All exact run-owned native processes stopped; no global installation, login, provider effects or inference.";
    // Preserve only content-free receipts in CI, not the private endpoint token
    // or synthetic profile. The test executable is also owned by this run.
    for (const name of [
      "data",
      "skill",
      "installer-store",
      "PR Sniper Control Acceptance.app",
      "pr-sniper.exe",
    ]) {
      await fs.rm(path.join(root, name), { recursive: true, force: true });
    }
  } catch (error) {
    receipt.cleanup = `FAILED: ${error.message}`;
    receipt.status = "FAIL";
    process.exitCode = 1;
  }
  await fs.writeFile(
    path.join(root, "receipt.json"),
    JSON.stringify(receipt, null, 2),
  );
  console.log(JSON.stringify(receipt, null, 2));
}
