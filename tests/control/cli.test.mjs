import test from "node:test";
import assert from "node:assert/strict";
import net from "node:net";
import {
  exchange,
  dataRoot,
} from "../../skills/pr-sniper/scripts/pr-sniper.mjs";

async function server(handler) {
  const listener = net.createServer(handler);
  await new Promise((resolve) => listener.listen(0, "127.0.0.1", resolve));
  return {
    endpoint: { port: listener.address().port, token: "a".repeat(64) },
    close: () => new Promise((resolve) => listener.close(resolve)),
  };
}

test("framed CLI sends the exact request and preserves typed native states", async () => {
  const request = {
    operation: "watch",
    request_id: "fixture",
    issued_at: 100,
    review_again: false,
  };
  const host = await server((socket) => {
    let input = Buffer.alloc(0);
    socket.on("data", (bytes) => {
      input = Buffer.concat([input, bytes]);
      if (input.length < 12 || input.length < input.readUInt32BE(8) + 12)
        return;
      assert.equal(input.subarray(0, 8).toString(), "SNIPER1\n");
      const envelope = JSON.parse(input.subarray(12));
      assert.equal(envelope.token, "a".repeat(64));
      assert.deepEqual(envelope.request, request);
      const payload = Buffer.from(
        JSON.stringify({
          protocol: 1,
          ok: true,
          result: { state: "waiting", paused: true },
        }),
      );
      const header = Buffer.alloc(4);
      header.writeUInt32BE(payload.length);
      socket.end(Buffer.concat([header, payload]));
    });
  });
  try {
    assert.deepEqual(await exchange(host.endpoint, request), {
      protocol: 1,
      ok: true,
      result: { state: "waiting", paused: true },
    });
  } finally {
    await host.close();
  }
});

test("lost replies are indeterminate and never retried by the transport", async () => {
  let calls = 0;
  const host = await server((socket) => {
    calls++;
    socket.once("data", () => socket.destroy());
  });
  try {
    await assert.rejects(
      exchange(host.endpoint, { request_id: "same-request" }),
      { code: "outcome_unknown" },
    );
    assert.equal(calls, 1);
  } finally {
    await host.close();
  }
});

test("oversized replies are rejected before unbounded buffering", async () => {
  const host = await server((socket) => {
    socket.once("data", () => {
      const header = Buffer.alloc(4);
      header.writeUInt32BE(1024 * 1024 + 1);
      socket.write(header);
    });
  });
  try {
    await assert.rejects(exchange(host.endpoint, {}), {
      code: "response_too_large",
    });
  } finally {
    await host.close();
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
