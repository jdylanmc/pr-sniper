// Synthetic transport: verifies the adapter, not live Copilot acceptance.
import { appendFileSync, existsSync } from "node:fs";
import { randomUUID } from "node:crypto";

if (process.env.TEST_STARTUP_DELAY_MS) {
  await new Promise((resolve) =>
    setTimeout(resolve, Number(process.env.TEST_STARTUP_DELAY_MS)),
  );
}

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
        if (scenario === "catalog-error") {
          frame({ jsonrpc: "2.0", id: request.id, error: {
            code: -32000, message: "Fixture catalog discovery failed",
          } });
          break;
        }
        respond(request.id, {
          models: [
            {
              id: "review-model", name: "Review model", capabilities: {},
              ...(scenario !== "unsupported-intelligence" ? {
                supportedReasoningEfforts: ["low", "high"],
                defaultReasoningEffort: "low",
                supportedContextTiers: ["default", "long_context"],
              } : {}),
            },
          ],
        });
        break;
      case "session.create":
        sessionId = request.params.sessionId;
        if (scenario === "reject-intelligence") {
          frame({ jsonrpc: "2.0", id: request.id, error: {
            code: -32602, message: "Fixture rejects requested intelligence",
          } });
          break;
        }
        globalThis.intelligence = {
          reasoningEffort: request.params.reasoningEffort,
          contextTier: request.params.contextTier,
        };
        respond(request.id, { sessionId });
        break;
      case "session.model.getCurrent":
        if (scenario === "intelligence-readback-error") {
          frame({ jsonrpc: "2.0", id: request.id, error: {
            code: -32000, message: "Fixture runtime readback unavailable",
          } });
          break;
        }
        respond(request.id, {
          modelId: "review-model",
          ...(scenario === "ignore-intelligence"
            ? { reasoningEffort: "low", contextTier: "default" }
            : globalThis.intelligence),
        });
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
        if (["final-full-review", "read-source-only", "tool-invalid-path"].includes(scenario)) {
          const toolName = scenario === "read-source-only" ? "read_source" : "read_changes";
          event("tool.execution_start", { toolName });
          event("external_tool.requested", {
            sessionId,
            requestId: "final-read",
            toolCallId: "final-read",
            toolName,
            arguments: scenario === "read-source-only"
              ? { path: "source.rs", side: "head" }
              : { paths: [scenario === "tool-invalid-path" ? "missing.rs" : "source.rs"] },
          });
          break;
        }
        if (scenario === "waiting") break;
        setTimeout(() => {
          if (["failure-held-abort", "runtime-error"].includes(scenario)) {
            event("session.error", {
              statusCode: 503, retryAfterSeconds: 7, errorType: "network",
              message: "ghp_secret-runtime-token SOURCE PRIVATE PROMPT",
            });
            return;
          }
          if (scenario === "runtime-truncation") {
            event("session.truncation", {
              messagesRemovedDuringTruncation: 3,
              tokenLimit: 123, privateContent: "DO NOT RECORD",
            });
            event("tool.execution_complete", { success: false, toolCallId: "remote-tool", error: { message: "DO NOT RECORD" } });
          }
          event("assistant.usage", { inputTokens: 20, outputTokens: 10 });
          event("assistant.message", {
            content: scenario.startsWith("malformed")
              ? "not JSON"
              : scenario.startsWith("follow-up-")
                ? JSON.stringify({
                    decision:
                      scenario === "follow-up-human"
                        ? "human_input_required"
                        : "quiet",
                    body: "",
                    new_information: "",
                    evidence: [],
                    reason:
                      "A human decision is required or no new evidence exists.",
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
      case "session.tools.handlePendingToolCall":
        receipt({ method: "fixture.finalRead", result: request.params.result });
        respond(request.id, {});
        if (["read-source-only", "tool-invalid-path"].includes(scenario)) {
          event("assistant.usage", { inputTokens: 20, outputTokens: 10 });
          event("assistant.message", { content: "{}" });
          event("session.idle", {});
          break;
        }
        if (
          scenario !== "final-full-review" ||
          request.params.requestId !== "final-read" ||
          !JSON.stringify(request.params.result ?? null).includes("answer")
        ) {
          event("session.error", { statusCode: 500 });
        } else {
          event("assistant.usage", { inputTokens: 20, outputTokens: 10 });
          event("assistant.message", {
            content: JSON.stringify({
              synopsis: "The synthetic source was reviewed completely.",
              files: [
                {
                  path: "source.rs",
                  explanation: "The full changed source was read.",
                  order: 1,
                },
              ],
              findings: [],
              decision: "machine_sign_off",
            }),
          });
          event("session.idle", {});
        }
        break;
      case "session.abort":
        if (scenario.endsWith("-held-abort")) {
          const held = setInterval(() => {
            if (!existsSync(process.env.TEST_ABORT_RELEASE)) return;
            clearInterval(held);
            receipt({ method: "fixture.abortReleased" });
            respond(request.id, {});
          }, 10);
          break;
        }
        respond(request.id, {});
        break;
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
