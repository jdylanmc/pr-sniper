import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { protocol } from "../../src-control/control-client.mjs";
import { fakeApp } from "./fake-app.mjs";

const source = path.resolve("src-tauri/resources/mcp/mcp-server.mjs");
// The exact native queue shape, pinned by the app's dispatcher tests.
const [queueItem] = JSON.parse(
  await fs.readFile(
    path.resolve("tests/control/fixtures/native-queue.json"),
    "utf8",
  ),
).values;
const AVERY = "00000000-0000-0000-0000-000000000001";
const AZURE_REPOSITORY = `contoso/00000000-0000-0000-0000-0000000000b1/00000000-0000-0000-0000-0000000000c1`;
// Native shapes pinned by the dispatcher and Azure DevOps setup tests.
const azureAccounts = {
  values: [
    {
      state: "connected",
      provider: "azure_devops",
      account_id: AVERY,
      login: "Avery Example",
      organization: "contoso",
      connection_generation: 0,
    },
  ],
  total: 1,
  next_offset: null,
  azure_devops: {
    cli: { state: "ready", version: "2.90.0" },
    minimum: "2.54.0",
    installer: "homebrew",
    organization: {
      state: "tenant_known",
      name: "contoso",
      tenant: "00000000-0000-0000-0000-00000000000a",
    },
    account: {
      state: "connected",
      identity: { id: AVERY, display_name: "Avery Example" },
    },
    checking: false,
  },
};
const azureResolution = {
  identity: { id: AVERY, login: "Avery Example" },
  repository: { id: AZURE_REPOSITORY, name: "contoso/Web/app" },
  account_generation: 0,
  pull_request: null,
};
async function fixture(mode) {
  const base = path.resolve("src-tauri/target");
  await fs.mkdir(base, { recursive: true });
  const root = await fs.mkdtemp(path.join(base, "mcp-adapter-"));
  await fs.mkdir(path.join(root, "control"), { mode: 0o700 });
  const calls = [];
  const token =
    randomUUID().replaceAll("-", "") + randomUUID().replaceAll("-", "");
  const listener = await fakeApp(token, (request) => {
    calls.push(request);
    const result =
      request.operation === "resources"
        ? {
            values: [],
            total: 0,
            next_offset: null,
            revision: "a".repeat(64),
          }
        : request.operation === "queue"
          ? {
              values: [queueItem],
              total: 1,
              next_offset: null,
            }
          : request.operation === "accounts"
            ? azureAccounts
            : request.operation === "resolve"
              ? azureResolution
              : request.operation === "watch"
                ? {
                    state: "waiting",
                    reason: "paused",
                    paused: true,
                    prioritized: 0,
                    item_id: null,
                    replayed: false,
                  }
                : null;
    return result
      ? { protocol, ok: true, result }
      : {
          protocol,
          ok: false,
          error: { code: "conflict", detail: "Synthetic native conflict." },
        };
  });
  await fs.writeFile(
    path.join(root, "control", "endpoint.json"),
    JSON.stringify({
      protocol,
      token,
      port: listener.port,
      app_version: "fixture",
      pid: process.pid,
    }),
    { mode: 0o600 },
  );
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [source],
    env: { PR_SNIPER_DATA_DIR: root },
    stderr: "pipe",
  });
  let stderr = "";
  transport.stderr?.on("data", (bytes) => {
    stderr = (stderr + bytes).slice(-16384);
  });
  const client = new Client(
    { name: "pr-sniper-acceptance", version: "1" },
    mode === "modern"
      ? { versionNegotiation: { mode: { pin: "2026-07-28" } } }
      : {},
  );
  try {
    await client.connect(transport, { timeout: 10000 });
  } catch (error) {
    await transport.close();
    await listener.close();
    await fs.rm(root, { recursive: true });
    throw new Error(`${error}; ${stderr}`);
  }
  return {
    client,
    calls,
    async close() {
      await client.close();
      await listener.close();
      await fs.rm(root, { recursive: true, force: true });
    },
  };
}

for (const mode of ["legacy", "modern"]) {
  test(`packaged MCP ${mode} stdio discovers typed tools without launching the app`, async () => {
    const f = await fixture(mode);
    try {
      const { tools } = await f.client.listTools();
      assert(tools.length > 30);
      assert(
        !tools.some((tool) =>
          ["execute", "login", "disconnect_account"].includes(tool.name),
        ),
      );
      const save = tools.find((tool) => tool.name === "save_agent");
      assert(
        save.inputSchema.properties.value.anyOf.some(
          (value) => value.type === "object" && value.properties.prompt,
        ),
      );
      assert(save.inputSchema.required.includes("request_id"));
      assert.equal(save.annotations.readOnlyHint, false);
      assert.equal(f.calls.length, 0);
      const prepared = await f.client.callTool({
        name: "prepare_request",
        arguments: {},
      });
      assert.match(prepared.structuredContent.request_id, /^[a-f0-9-]{36}$/);
      assert.equal(f.calls.length, 0);
      const read = await f.client.callTool({
        name: "list_doctrines",
        arguments: { limit: 2 },
      });
      assert.equal(read.isError, false, JSON.stringify(read));
      assert.deepEqual(read.structuredContent.result.values, []);
      assert.equal(f.calls[0].operation, "resources");
      assert.equal(f.calls[0].kind, "doctrines");
      assert.equal(f.calls[0].limit, 2);
      const before = f.calls.length;
      const invalid = await f.client.callTool({
        name: "save_agent",
        arguments: { unexpected: "must not reach native" },
      });
      assert.equal(invalid.isError, true);
      assert.equal(f.calls.length, before);
    } finally {
      await f.close();
    }
  });
}

for (const mode of ["modern", "legacy"]) {
  test(`MCP queue reports exits, settled findings, Duck reasons and retry times (${mode})`, async () => {
    const f = await fixture(mode);
    try {
      const response = await f.client.callTool({
        name: "list_queue",
        arguments: {},
      });
      assert.equal(response.isError, false, JSON.stringify(response));
      assert.deepEqual(response.structuredContent.result.values, [queueItem]);
    } finally {
      await f.close();
    }
  });
}

test("MCP preserves native conflicts and retry identity without retrying effects", async () => {
  const f = await fixture("legacy");
  try {
    const { structuredContent: request } = await f.client.callTool({
      name: "prepare_request",
      arguments: {},
    });

    const id = randomUUID();
    const response = await f.client.callTool({
      name: "save_doctrine",
      arguments: {
        ...request,
        revision: "a".repeat(64),
        id,
        expected: null,
        value: {
          id,
          title: "Fixture",
          body: "Do not overwrite native conflicts.",
        },
      },
    });
    assert.equal(response.isError, true);
    assert(response.structuredContent, JSON.stringify(response));
    assert.equal(response.structuredContent.error.code, "conflict");
    assert.equal(f.calls.length, 1);
    assert.equal(f.calls[0].request_id, request.request_id);
    assert.equal(f.calls[0].issued_at, request.issued_at);
    assert.equal(f.calls[0].edit.kind, "doctrine");
  } finally {
    await f.close();
  }
});

test("MCP watch preserves exact scope, pause, override and explicit rerun intent", async () => {
  const f = await fixture("modern");
  try {
    const expected = {
      id: randomUUID(),
      provider: "github",
      name: "owner/repo",
      enabled: true,
      provider_account_id: "account",
      provider_repository_id: "repository",
      admission: { mode: "explicit", rules: [] },
      assignments: [],
    };
    const overrides = {
      [randomUUID()]: { agent_id: randomUUID(), doctrine_ids: null },
    };
    const identity = (
      await f.client.callTool({ name: "prepare_request", arguments: {} })
    ).structuredContent;
    const args = {
      ...identity,
      expected,
      url: "https://github.com/owner/repo/pull/7",
      account_generation: 8,
      review_again: true,
      overrides,
    };
    const response = await f.client.callTool({
      name: "watch_pull_request",
      arguments: args,
    });
    assert.equal(response.isError, false, JSON.stringify(response));
    assert.equal(response.structuredContent.result.state, "waiting");
    assert.equal(response.structuredContent.result.paused, true);
    assert.deepEqual(f.calls, [{ operation: "watch", ...args }]);
    const rejected = await f.client.callTool({
      name: "watch_pull_request",
      arguments: { ...args, allow_grants: true },
    });
    assert.equal(rejected.isError, true);
    assert.equal(
      f.calls.length,
      1,
      "Unsupported grant expansion never reaches native watch",
    );
  } finally {
    await f.close();
  }
});

test("MCP lists, resolves and saves Azure DevOps repositories with setup state", async () => {
  const f = await fixture("modern");
  try {
    const accounts = await f.client.callTool({
      name: "list_repository_accounts",
      arguments: {},
    });
    assert.equal(accounts.isError, false, JSON.stringify(accounts));
    assert.deepEqual(accounts.structuredContent.result, azureAccounts);
    const { operation, kind } = f.calls.at(-1);
    assert.deepEqual(
      { operation, kind },
      { operation: "accounts", kind: "repository" },
    );
    const url = "https://dev.azure.com/contoso/Web/_git/app";
    const resolved = await f.client.callTool({
      name: "resolve_repository",
      arguments: { account_id: AVERY, url },
    });
    assert.equal(resolved.isError, false, JSON.stringify(resolved));
    assert.deepEqual(resolved.structuredContent.result, azureResolution);
    assert.equal(f.calls.at(-1).operation, "resolve");
    assert.equal(f.calls.at(-1).account_id, AVERY);
    assert.equal(f.calls.at(-1).url, url);
    const id = randomUUID();
    const value = {
      id,
      provider: "azure_devops",
      name: "contoso/Web/app",
      enabled: false,
      provider_account_id: AVERY,
      provider_repository_id: AZURE_REPOSITORY,
      watched_authors: [
        { id: "00000000-0000-0000-0000-000000000002", login: "Blake Example" },
      ],
    };
    const validate = await f.client.callTool({
      name: "validate_repository",
      arguments: { id, expected: null, value },
    });
    // The fixture app answers validation with a conflict; the request reached it.
    assert.equal(validate.isError, true);
    assert.equal(f.calls.at(-1).operation, "validate");
    assert.deepEqual(f.calls.at(-1).edit, {
      kind: "repository",
      id,
      expected: null,
      value,
    });
    const request = (
      await f.client.callTool({ name: "prepare_request", arguments: {} })
    ).structuredContent;
    await f.client.callTool({
      name: "save_repository",
      arguments: {
        ...request,
        revision: "a".repeat(64),
        id,
        expected: null,
        value,
        account_generation: 0,
      },
    });
    assert.deepEqual(f.calls.at(-1).edit, {
      kind: "repository",
      id,
      expected: null,
      value,
    });
    assert.equal(f.calls.at(-1).account_generation, 0);
  } finally {
    await f.close();
  }
});
