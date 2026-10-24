// Test double for the native listener's mutually authenticated framing. The
// real server is src-tauri/src/control/transport.rs; KATs pin both to the
// same proofs.
import net from "node:net";
import { randomBytes } from "node:crypto";
import {
  proof,
  proofMatches,
  protocol,
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
            proof: proof(token, "server", hello.nonce, server),
          },
          false,
        );
        return;
      }
      if (
        !proofMatches(body.proof, proof(token, "client", hello.nonce, server))
      )
        return reply(socket, {
          protocol,
          ok: false,
          error: { code: "unauthorized", detail: null },
        });
      const value = handler(body.request, socket);
      if (value) reply(socket, value);
    });
  });
  await new Promise((resolve) => listener.listen(0, "127.0.0.1", resolve));
  return {
    port: listener.address().port,
    close: () => new Promise((resolve) => listener.close(resolve)),
  };
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
