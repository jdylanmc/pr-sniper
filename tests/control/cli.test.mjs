import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { exchange, dataRoot, invoke } from "../../src-control/pr-sniper.mjs";
import {
  mac,
  proofMatches,
  protocol,
  sha256,
  useExecutable,
} from "../../src-control/control-client.mjs";
import { fakeApp, flip, impostor, relay, reply, signed } from "./fake-app.mjs";

const token = "a".repeat(64);

test("framed CLI authenticates the app, sends the exact request and preserves typed native states", async () => {
  const request = {
    operation: "watch",
    request_id: "fixture",
    issued_at: 100,
    review_again: false,
  };
  const seen = [];
  const host = await fakeApp(token, (input) => {
    seen.push(input);
    return { protocol, ok: true, result: { state: "waiting", paused: true } };
  });
  try {
    assert.deepEqual(await exchange({ port: host.port, token }, request), {
      protocol,
      ok: true,
      result: { state: "waiting", paused: true },
    });
    assert.deepEqual(seen, [request]);
  } finally {
    await host.close();
  }
});

test("lost replies are indeterminate and never retried by the transport", async () => {
  let calls = 0;
  const host = await fakeApp(token, (_, socket) => {
    calls++;
    socket.destroy();
  });
  try {
    await assert.rejects(
      exchange({ port: host.port, token }, { request_id: "same-request" }),
      { code: "outcome_unknown" },
    );
    assert.equal(calls, 1);
  } finally {
    await host.close();
  }
});

test("oversized replies are rejected before unbounded buffering", async () => {
  const host = await fakeApp(token, (_, socket) => {
    const header = Buffer.alloc(4);
    header.writeUInt32BE(1024 * 1024 + 1);
    socket.write(header);
  });
  try {
    await assert.rejects(exchange({ port: host.port, token }, {}), {
      code: "response_too_large",
    });
  } finally {
    await host.close();
  }
});

test("proofs are domain-separated HMAC-SHA256 pinned to the native server", () => {
  const [client, server] = ["b".repeat(64), "c".repeat(64)];
  const rq = sha256(Buffer.from('{"operation":"inspect"}'));
  const rs = sha256(Buffer.from('{"protocol":2,"ok":true}'));
  // Same vectors as control_proofs_are_domain_separated_hmac_sha256 (Rust).
  assert.equal(
    mac(token, "challenge", client, server),
    "9be37531fcc3b4c3250e1bf788cf8a278d76e86925f29d220037a323b19dbf85",
  );
  assert.equal(
    mac(token, "request", client, server, rq),
    "840373773afcbfc2b5e109150d3aec8cc5c46d8683cdee8f21d5511a744006f5",
  );
  assert.equal(
    mac(token, "response", client, server, rq, rs),
    "55cf3d2ee068f8ffa08852174a0ef68dd9f440b9865489def76c7468cbae1c1d",
  );
  const valid = mac(token, "challenge", client, server);
  assert.equal(proofMatches(valid, valid), true);
  for (const forged of [
    mac(token, "request", client, server),
    valid.toUpperCase(),
    valid.slice(2),
    `${valid}00`,
    undefined,
    token,
  ])
    assert.equal(proofMatches(forged, valid), false);
});

test("a relay that changes one request byte is rejected before native dispatch", async () => {
  const seen = [];
  const app = await fakeApp(token, (input) => {
    seen.push(input);
    return { protocol, ok: true, result: { state: "saved" } };
  });
  const request = {
    operation: "save",
    request_id: "relayed",
    edit: { value: "replacement" },
  };
  // Forwards the genuine challenge and request proof; alters the request.
  const proxy = await relay(app.port, (direction, index, body) =>
    direction === "request" && index === 1
      ? { ...body, request: flip(body.request, "t") }
      : null,
  );
  try {
    await assert.rejects(exchange({ port: proxy.port, token }, request), {
      code: "outcome_unknown",
    });
    assert.deepEqual(seen, []);
  } finally {
    await proxy.close();
    await app.close();
  }
});

for (const [name, mutate] of [
  [
    "changes one response byte",
    (body) => ({ ...body, response: flip(body.response, "f") }),
  ],
  [
    "replaces the signed response with plain success",
    () => ({ protocol, ok: true, result: { state: "saved" } }),
  ],
  [
    "re-signs a forged response without the capability",
    (body) => ({
      ...signed("f".repeat(64), "b".repeat(64), "c".repeat(64), sha256(""), {
        protocol,
        ok: true,
        result: { state: "saved" },
      }),
      proof: body.proof,
    }),
  ],
])
  test(`a relay that ${name} is never trusted as native success`, async () => {
    let calls = 0;
    const app = await fakeApp(token, () => {
      calls++;
      return {
        protocol,
        ok: false,
        error: { code: "conflict", detail: null },
      };
    });
    const proxy = await relay(app.port, (direction, index, body) =>
      direction === "response" && index === 1 ? mutate(body) : null,
    );
    try {
      await assert.rejects(
        exchange({ port: proxy.port, token }, { request_id: "relayed" }),
        { code: "outcome_unknown", detail: { request_id: "relayed" } },
      );
      assert.equal(calls, 1);
    } finally {
      await proxy.close();
      await app.close();
    }
  });

test("an unmodified relay path still exchanges authenticated frames", async () => {
  const app = await fakeApp(token, (input) => ({
    protocol,
    ok: true,
    result: input,
  }));
  const proxy = await relay(app.port, () => null);
  try {
    assert.deepEqual(
      await exchange({ port: proxy.port, token }, { operation: "inspect" }),
      { protocol, ok: true, result: { operation: "inspect" } },
    );
  } finally {
    await proxy.close();
    await app.close();
  }
});

for (const [name, respond] of [
  [
    "claims native success",
    (socket) =>
      reply(socket, { protocol, ok: true, result: { state: "saved" } }),
  ],
  [
    "replays a recorded server proof",
    (socket) => {
      // Valid for an earlier connection's client nonce, not this fresh one.
      const nonce = "c".repeat(64);
      reply(
        socket,
        {
          protocol,
          nonce,
          proof: mac(token, "challenge", "d".repeat(64), nonce),
        },
        false,
      );
    },
  ],
  ["stays silent", null],
])
  test(`an impostor listener that ${name} receives neither capability nor request`, async () => {
    const fake = await impostor(0, respond);
    const request = {
      operation: "save",
      request_id: "secret-request",
      edit: { expected: "secret-expected", value: "secret-replacement" },
    };
    try {
      await assert.rejects(
        exchange({ port: fake.port, token }, request, 120_000),
        { code: "unavailable" },
      );
      const bytes = fake.received();
      assert.equal(bytes.subarray(0, 8).toString(), "SNIPER1\n");
      assert.equal(bytes.length, bytes.readUInt32BE(8) + 12, "Only one frame");
      const hello = JSON.parse(bytes.subarray(12));
      assert.deepEqual(Object.keys(hello).sort(), ["nonce", "protocol"]);
      assert.match(hello.nonce, /^[a-f0-9]{64}$/);
      assert.doesNotMatch(bytes.toString(), /a{64}|secret|save/);
    } finally {
      await fake.close();
    }
  });

async function profile(t) {
  const base = path.resolve("src-tauri/target");
  await fs.mkdir(base, { recursive: true });
  const root = await fs.mkdtemp(path.join(base, "control-client-"));
  await fs.mkdir(path.join(root, "control"), { mode: 0o700 });
  const saved = { ...process.env };
  process.env.PR_SNIPER_DATA_DIR = root;
  process.env.PR_SNIPER_KEYCHAIN_SERVICE =
    "com.jdylanmc.pr-sniper.tests.client";
  // The "app" exits immediately; the test publishes the relaunched endpoint.
  useExecutable(process.execPath);
  t.after(async () => {
    process.env = saved;
    await fs.rm(root, { recursive: true, force: true });
  });
  const write = (value) =>
    fs.writeFile(
      path.join(root, "control", "endpoint.json"),
      JSON.stringify({ protocol, app_version: "fixture", ...value }),
      { mode: 0o600 },
    );
  return write;
}

test("a crashed app's descriptor with an impostor on its port relaunches the real app", async (t) => {
  const write = await profile(t);
  const real = "e".repeat(64);
  const seen = [];
  const app = await fakeApp(real, (input) => {
    seen.push(input);
    return { protocol, ok: true, result: { state: "saved" } };
  });
  const fake = await impostor(0, (socket) => {
    // The client reached the stale port: the relaunched app publishes anew.
    write({ port: app.port, token: real, pid: process.pid });
    reply(socket, { protocol, ok: true, result: { state: "saved" } });
  });
  // Live PID so the client must rely on the server proof, not liveness.
  await write({ port: fake.port, token, pid: process.pid });
  try {
    const response = await invoke({
      operation: "save",
      edit: { expected: "secret-expected", value: "secret-replacement" },
    });
    assert.equal(response.ok, true);
    assert.equal(seen.length, 1, "Real app received the request once");
    assert.equal(seen[0].edit.value, "secret-replacement");
    assert.doesNotMatch(fake.received().toString(), /a{64}|e{64}|secret/);
  } finally {
    await fake.close();
    await app.close();
  }
});

test("a descriptor whose process exited is stale and never contacted", async (t) => {
  const write = await profile(t);
  const exited = spawnSync(process.execPath, ["-e", ""]).pid;
  const app = await fakeApp(token, () => ({ protocol, ok: true, result: {} }));
  let contacted = false;
  const fake = await impostor(0, () => {
    contacted = true;
  });
  await write({ port: fake.port, token, pid: exited });
  const relaunch = setTimeout(
    () => write({ port: app.port, token, pid: process.pid }),
    100,
  );
  try {
    assert.equal((await invoke({ operation: "inspect" })).ok, true);
    assert.equal(contacted, false);
  } finally {
    clearTimeout(relaunch);
    await fake.close();
    await app.close();
  }
});

test("a withdrawn descriptor after a fatal app exit relaunches a fresh app", async (t) => {
  const write = await profile(t);
  const app = await fakeApp(token, (input) => ({
    protocol,
    ok: true,
    result: input,
  }));
  // No endpoint.json: the old process withdrew it before exiting.
  const relaunch = setTimeout(
    () => write({ port: app.port, token, pid: process.pid }),
    100,
  );
  try {
    const reply = await invoke({ operation: "inspect" });
    assert.equal(reply.ok, true);
    assert.equal(reply.result.operation, "inspect");
  } finally {
    clearTimeout(relaunch);
    await app.close();
  }
});

test("relative isolated profiles fail rather than selecting production data", () => {
  assert.throws(() => dataRoot({ PR_SNIPER_DATA_DIR: "relative" }), {
    code: "invalid_data_directory",
  });

  assert.equal(
    dataRoot({}, "darwin", "/fixture"),
    "/fixture/Library/Application Support/com.jdylanmc.pr-sniper",
  );
});

test("target-platform paths preserve Windows drives and reject drive-relative profiles", () => {
  assert.equal(
    dataRoot({ LOCALAPPDATA: "C:\\Users\\fixture\\AppData\\Local" }, "win32"),
    "C:\\Users\\fixture\\AppData\\Local\\com.jdylanmc.pr-sniper",
  );
  for (const root of ["C:\\data", "D:/data", "\\\\server\\share\\data"]) {
    assert.equal(dataRoot({ PR_SNIPER_DATA_DIR: root }, "win32"), root);
  }
  for (const root of ["C:data", "\\data", "/data", "relative"]) {
    assert.throws(() => dataRoot({ PR_SNIPER_DATA_DIR: root }, "win32"), {
      code: "invalid_data_directory",
    });
  }
  assert.throws(() => dataRoot({ LOCALAPPDATA: "relative" }, "win32"), {
    code: "invalid_data_directory",
  });
});
