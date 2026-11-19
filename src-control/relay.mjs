// Self-heal after an in-place app update (ADR 0025 amendment). A host keeps
// one stdio server per session; when the bundle it was started from is
// replaced, requests move to a proven-ready child running the new bundle and
// the host is told to refetch tools. The process holding the stdio wire stays.
import fs from "node:fs";
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import readline from "node:readline";

export const RELAYED = "PR_SNIPER_MCP_RELAYED";
const readyTimeout = 10_000;
const envelopeKey = "io.modelcontextprotocol/protocolVersion";

const fingerprint = (stat) =>
  `${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeMs}`;
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");

/** Content digest of `file`, re-hashed only when its stat identity changes. */
export function bundleWatch(file) {
  let mark;
  let digest = null;
  try {
    mark = fingerprint(fs.statSync(file));
    digest = hash(fs.readFileSync(file));
  } catch {
    /* unreadable at start: nothing to compare */
  }
  const started = digest;
  return {
    started,
    current() {
      if (started === null) return null;
      try {
        const next = fingerprint(fs.statSync(file));
        if (next !== mark) {
          digest = hash(fs.readFileSync(file));
          mark = next;
        }
      } catch {
        /* removed or mid-replacement: keep the last observation */
      }
      return digest;
    },
    stale() {
      const now = this.current();
      return now !== null && now !== started;
    },
  };
}

const isRequest = (m) => typeof m?.method === "string" && m.id !== undefined;
const isResponse = (m) => m?.method === undefined && m?.id !== undefined;

/**
 * Wraps the stdio wire. `tools/list` and `tools/call` check the bundle; on a
 * new digest a child is started, initialized and proven with `tools/list`
 * before any client request reaches it. Until then, or if the child fails,
 * the in-process server answers (reads stay lenient; mutations are refused).
 */
export function relayTransport(wire, { watch, command, onSwitch }) {
  let active = { digest: watch.started, child: null };
  const retired = new Set();
  const failed = new Set();
  let initialize;
  let initialized = false;
  let envelope;
  let switching = null;
  const buffered = [];
  const self = {
    onmessage: undefined,
    onclose: undefined,
    onerror: undefined,
    async start() {
      wire.onmessage = (message, extra) => inbound(message, extra);
      wire.onerror = (error) => self.onerror?.(error);
      wire.onclose = () => {
        for (const backend of [active, ...retired]) backend.child?.end();
        self.onclose?.();
      };
      return wire.start();
    },
    send: (message, options) => wire.send(message, options),
    setProtocolVersion: (version) => wire.setProtocolVersion?.(version),
    async close() {
      for (const backend of [active, ...retired]) backend.child?.end();
      return wire.close();
    },
  };

  const local = (message, extra) => self.onmessage?.(message, extra);

  function inbound(message, extra) {
    if (message?.method === "initialize" && isRequest(message))
      initialize = message;
    if (message?.method === "notifications/initialized") initialized = true;
    if (message?.params?._meta?.[envelopeKey]) envelope = message.params._meta;
    if (switching) return void buffered.push([message, extra]);
    if (
      isRequest(message) &&
      ["tools/list", "tools/call"].includes(message.method)
    ) {
      const digest = watch.current();
      if (digest && digest !== active.digest && !failed.has(digest)) {
        buffered.push([message, extra]);
        switching = upgrade(digest).finally(() => {
          switching = null;
          for (const [next, nextExtra] of buffered.splice(0))
            inbound(next, nextExtra);
        });
        return;
      }
    }
    route(message, extra);
  }

  function route(message, extra) {
    if (isRequest(message) && message.method !== "subscriptions/listen") {
      if (active.child) return active.child.forward(message);
      return local(message, extra);
    }
    if (message?.method === "notifications/cancelled") {
      const id = message.params?.requestId;
      for (const backend of [active, ...retired])
        if (backend.child?.owns(id)) return backend.child.forward(message);
      return local(message, extra);
    }
    if (isResponse(message))
      for (const backend of [active, ...retired])
        if (backend.child?.asked(message.id))
          return backend.child.forward(message);
    local(message, extra);
  }

  async function upgrade(digest) {
    const child = await start(digest);
    if (!child) return void failed.add(digest);
    const previous = active;
    active = { digest, child };
    if (previous.child) {
      retired.add(previous);
      previous.child.drain(() => retired.delete(previous));
    }
    onSwitch?.();
  }

  function start(digest) {
    const [file, ...args] = command;
    let proc;
    try {
      proc = spawn(process.execPath, [file, ...args], {
        stdio: ["pipe", "pipe", "inherit"],
        env: { ...process.env, [RELAYED]: "1" },
        windowsHide: true,
      });
    } catch {
      return Promise.resolve(null);
    }
    const pending = new Set();
    const asked = new Set();
    const hidden = new Map();
    let drained;
    let alive = true;
    const write = (message) => {
      if (alive && proc.stdin.writable)
        proc.stdin.write(`${JSON.stringify(message)}\n`);
    };
    const settle = (id) => {
      pending.delete(id);
      if (drained && pending.size === 0) {
        proc.stdin.end();
        drained();
      }
    };
    const child = {
      forward(message) {
        if (isRequest(message)) pending.add(message.id);
        if (isResponse(message)) asked.delete(message.id);
        if (message.method === "notifications/cancelled")
          settle(message.params?.requestId);
        write(message);
      },
      owns: (id) => pending.has(id),
      asked: (id) => asked.has(id),
      end: () => proc.stdin.end(),
      drain(done) {
        drained = done;
        if (pending.size === 0) {
          proc.stdin.end();
          done();
        }
      },
    };
    proc.stdin.on("error", () => {});
    const call = (method, params) =>
      new Promise((resolve) => {
        const id = `pr-sniper-relay:${randomUUID()}`;
        hidden.set(id, resolve);
        write({ jsonrpc: "2.0", id, method, params });
      });
    readline.createInterface({ input: proc.stdout }).on("line", (line) => {
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        return;
      }
      if (isResponse(message) && hidden.has(message.id)) {
        hidden.get(message.id)(message);
        return void hidden.delete(message.id);
      }
      if (isResponse(message)) settle(message.id);
      else if (isRequest(message)) asked.add(message.id);
      wire.send(message).catch((error) => self.onerror?.(error));
    });
    return new Promise((resolve) => {
      const fail = () => {
        alive = false;
        proc.kill();
        resolve(null);
      };
      const timer = setTimeout(fail, readyTimeout);
      proc.once("error", fail);
      proc.once("exit", () => {
        alive = false;
        clearTimeout(timer);
        resolve(null);
        // Requests already in the child cannot be re-run elsewhere.
        for (const id of pending)
          wire
            .send({
              jsonrpc: "2.0",
              id,
              error: {
                code: -32603,
                message:
                  "pr-sniper MCP backend exited; outcome unknown. Reload the pr-sniper MCP server.",
              },
            })
            .catch(() => {});
        pending.clear();
        if (active.child === child)
          active = { digest: active.digest, child: null };
      });
      (async () => {
        if (!envelope) {
          if (!initialize) return fail();
          const reply = await call("initialize", initialize.params);
          if (reply.error) return fail();
          if (initialized)
            write({ jsonrpc: "2.0", method: "notifications/initialized" });
        }
        const listed = await call(
          "tools/list",
          envelope ? { _meta: envelope } : {},
        );
        clearTimeout(timer);
        if (!alive || listed.error || !Array.isArray(listed.result?.tools))
          return fail();
        resolve(child);
      })();
    });
  }

  return self;
}
