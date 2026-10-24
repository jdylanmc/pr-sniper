import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { exchange, dataRoot, invoke } from "../../src-control/pr-sniper.mjs";
import {
  proof,
  proofMatches,
  protocol,
  useExecutable,
} from "../../src-control/control-client.mjs";
import { fakeApp, impostor, reply } from "./fake-app.mjs";

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
  // Same vectors as control_proofs_are_domain_separated_hmac_sha256 (Rust).
  assert.equal(
    proof(token, "server", client, server),
    "a83260e7589091cf80635cc6b9b516846f8a12625769899220f850f578146f8b",
  );
  assert.equal(
    proof(token, "client", client, server),
    "11fb03febf717d5898d5e3caeca82858bc5a77144b445f4287eefbb24129e530",
  );
  const valid = proof(token, "server", client, server);
  assert.equal(proofMatches(valid, valid), true);
  for (const forged of [
    proof(token, "client", client, server),
    valid.toUpperCase(),
    valid.slice(2),
    `${valid}00`,
    undefined,
    token,
  ])
    assert.equal(proofMatches(forged, valid), false);
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
          proof: proof(token, "server", "d".repeat(64), nonce),
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
