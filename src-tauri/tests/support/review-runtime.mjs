// Synthetic transport: verifies the adapter, not live Copilot acceptance.
import { appendFileSync } from "node:fs";
import { randomUUID } from "node:crypto";

const scenario = process.env.REVIEW_SCENARIO;
const receipt = (value) =>
  appendFileSync(process.env.TEST_RECEIPT, `${JSON.stringify(value)}\n`);
const frame = (value) => {
  const body = JSON.stringify(value);
  process.stdout.write(
    `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`,
  );
};
const respond = (id, result) => frame({ jsonrpc: "2.0", id, result });
let sessionId;
const event = (type, data) =>
  frame({
    jsonrpc: "2.0",
    method: "session.event",
    params: {
      sessionId,
      event: {
        id: randomUUID(),
        timestamp: new Date().toISOString(),
        type,
        data,
      },
    },
  });
receipt({
  pid: process.pid,
  ambient: [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "COPILOT_GITHUB_TOKEN",
    "NODE_OPTIONS",
  ].filter((k) => process.env[k]),
});
let input = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  input = Buffer.concat([input, chunk]);
  while (true) {
    const header = input.indexOf("\r\n\r\n");
    if (header < 0) return;
    const length = Number(
      /Content-Length:\s*(\d+)/i.exec(input.subarray(0, header).toString())[1],
    );
    if (input.length < header + 4 + length) return;
    const request = JSON.parse(input.subarray(header + 4, header + 4 + length));
    input = input.subarray(header + 4 + length);
    receipt({
      method: request.method,
      ...(request.method === "session.create"
        ? { config: request.params }
        : {}),
    });
    if (request.id === undefined) continue;
    switch (request.method) {
      case "connect":
        respond(request.id, {
          ok: true,
          protocolVersion: 3,
          version: "synthetic",
        });
        break;
      case "ping":
        respond(request.id, {
          protocolVersion: 3,
          message: "pong",
          timestamp: new Date().toISOString(),
        });
        break;
      case "auth.getStatus":
        respond(request.id, {
          isAuthenticated: true,
          authType: "token",
          login: scenario === "wrong-account" ? "other" : "review-account",
        });
        break;
      case "status.get":
        respond(request.id, { version: "synthetic", protocolVersion: 3 });
        break;
      case "models.list":
        respond(request.id, {
          models: [
            { id: "review-model", name: "Review model", capabilities: {} },
          ],
        });
        break;
      case "session.create":
        sessionId = request.params.sessionId;
        respond(request.id, { sessionId });
        break;
      case "session.eventLog.registerInterest":
        respond(request.id, { id: randomUUID() });
        break;
      case "session.tools.initializeAndValidate":
        respond(request.id, {});
        break;
      case "session.options.update":
        respond(request.id, { success: true });
        break;
      case "session.tools.getCurrentMetadata":
        respond(request.id, {
          tools: [
            ...["read_changes", "read_source", "search_paths"],
            ...(scenario === "extra-tool" ? ["bash"] : []),
          ].map((name) => ({ name, description: name })),
        });
        break;
      case "session.send":
        respond(request.id, { messageId: randomUUID() });
        if (scenario === "waiting") break;
        setTimeout(() => {
          event("assistant.usage", { inputTokens: 20, outputTokens: 10 });
          event("assistant.message", {
            content:
              scenario === "malformed"
                ? "not JSON"
                : scenario.startsWith("follow-up-")
                  ? JSON.stringify({
                      decision: scenario === "follow-up-human" ? "human_input_required" : "quiet",
                      body: "", new_information: "", evidence: [],
                      reason: "A human decision is required or no new evidence exists.",
                    })
                : JSON.stringify({
                    synopsis: "The empty synthetic change has no findings.",
                    files: [],
                    findings: [],
                    decision: "machine_sign_off",
                  }),
          });
          event("session.idle", {});
        }, 10);
        break;
      case "session.abort":
      case "session.destroy":
      case "session.detach":
      case "runtime.shutdown":
        respond(request.id, {});
        break;
      default:
        frame({
          jsonrpc: "2.0",
          id: request.id,
          error: {
            code: -32601,
            message: `Unexpected fixture method: ${request.method}`,
          },
        });
    }
  }
});
