// Test double for the native listener's mutually authenticated framing. The
// real server is src-tauri/src/control/transport.rs; KATs pin both to the
// same proofs.
import net from "node:net";
import { randomBytes } from "node:crypto";
import {
  mac,
  proofMatches,
  protocol,
  sha256,
} from "../../src-control/control-client.mjs";

function frames(socket, onFrame) {
  let input = Buffer.alloc(0);
  socket.on("error", () => {});
  socket.on("data", (bytes) => {
    input = Buffer.concat([input, bytes]);
    while (input.length >= 12 && input.length >= input.readUInt32BE(8) + 12) {
      const length = input.readUInt32BE(8);
      if (input.subarray(0, 8).toString() !== "SNIPER1\n")
        return socket.destroy();
      const body = JSON.parse(input.subarray(12, length + 12));
      input = input.subarray(length + 12);
      onFrame(body);
    }
  });
}

export function reply(socket, value, end = true) {
  const payload = Buffer.from(JSON.stringify(value));
  const header = Buffer.alloc(4);
  header.writeUInt32BE(payload.length);
  socket[end ? "end" : "write"](Buffer.concat([header, payload]));
}

/** Signed response frame exactly as the native server writes it. */
export function signed(token, client, server, digest, value) {
  const raw = Buffer.from(JSON.stringify(value));
  return {
    protocol,
    proof: mac(token, "response", client, server, digest, sha256(raw)),
    response: raw.toString("base64"),
  };
}

/** `handler(request, socket)` returns a reply object, or handles the socket itself. */
export async function fakeApp(token, handler) {
  const listener = net.createServer((socket) => {
    let hello;
    let server;
    frames(socket, (body) => {
      if (!hello) {
        hello = body;
        server = randomBytes(32).toString("hex");
        reply(
          socket,
          {
            protocol,
            nonce: server,
            proof: mac(token, "challenge", hello.nonce, server),
          },
          false,
        );
        return;
      }
      const raw = Buffer.from(body.request ?? "", "base64");
      const digest = sha256(raw);
      const respond = (value) =>
        reply(socket, signed(token, hello.nonce, server, digest, value));
      if (
        !proofMatches(
          body.proof,
          mac(token, "request", hello.nonce, server, digest),
        )
      )
        return respond({
          protocol,
          ok: false,
          error: { code: "unauthorized", detail: null },
        });
      const value = handler(JSON.parse(raw), socket);
      if (value) respond(value);
    });
  });
  await new Promise((resolve) => listener.listen(0, "127.0.0.1", resolve));
  return {
    port: listener.address().port,
    close: () => new Promise((resolve) => listener.close(resolve)),
  };
}

/**
 * Active relay between a client and an upstream listener. `mutate(direction,
 * index, json)` may return a replacement frame body.
 */
export async function relay(upstream, mutate) {
  const listener = net.createServer((inbound) => {
    const outbound = net.createConnection({
      host: "127.0.0.1",
      port: upstream,
    });
    inbound.on("error", () => outbound.destroy());
    outbound.on("error", () => inbound.destroy());
    inbound.on("close", () => outbound.destroy());
    outbound.on("close", () => inbound.destroy());
    const pump = (from, to, direction, headerSize) => {
      let input = Buffer.alloc(0);
      let index = 0;
      from.on("data", (bytes) => {
        input = Buffer.concat([input, bytes]);
        while (
          input.length >= headerSize &&
          input.length >= input.readUInt32BE(headerSize - 4) + headerSize
        ) {
          const length = input.readUInt32BE(headerSize - 4);
          const body = JSON.parse(
            input.subarray(headerSize, length + headerSize),
          );
          input = input.subarray(length + headerSize);
          const payload = Buffer.from(
            JSON.stringify(mutate(direction, index++, body) ?? body),
          );
          const header = Buffer.alloc(headerSize);
          if (headerSize === 12) Buffer.from("SNIPER1\n").copy(header);
          header.writeUInt32BE(payload.length, headerSize - 4);
          to.write(Buffer.concat([header, payload]));
        }
      });
    };
    pump(inbound, outbound, "request", 12);
    pump(outbound, inbound, "response", 4);
  });
  await new Promise((resolve) => listener.listen(0, "127.0.0.1", resolve));
  return {
    port: listener.address().port,
    close: () => new Promise((resolve) => listener.close(resolve)),
  };
}

/** Flips one byte inside a base64 field's decoded bytes. */
export function flip(field, pattern) {
  const raw = Buffer.from(field, "base64");
  const index = raw.lastIndexOf(pattern);
  if (index < 0) throw new Error(`No ${pattern} to flip`);
  raw[index] ^= 0x20;
  return raw.toString("base64");
}

/** Records every byte an unauthenticated listener receives; optionally lies. */
export async function impostor(port, respond) {
  const received = [];
  const listener = net.createServer((socket) => {
    socket.on("error", () => {});
    socket.on("data", (bytes) => {
      received.push(bytes);
      if (respond) {
        respond(socket);
        respond = null;
      }
    });
  });
  await new Promise((resolve, reject) => {
    listener.once("error", reject);
    listener.listen(port, "127.0.0.1", resolve);
  });
  return {
    port: listener.address().port,
    received: () => Buffer.concat(received),
    close: () => new Promise((resolve) => listener.close(resolve)),
  };
}
