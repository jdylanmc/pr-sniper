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
const exitScenarios = [
  "exit-ready", "exit-comments", "exit-denied-then-ok", "exit-none", "exit-double",
  "exit-malformed-forever", "exit-catalog-mismatch", "panic", "report-ok", "report-too-long",
  "exit-approve", "exit-merge", "exit-approve-denied-self",
  "verdict-ok", "verdict-revise-bad-then-ok",
];
// Primary scenarios that call rubber_duck: [calls in parallel, args].
const duckArgs = { kind: "line", path: "source.rs", line: 1, claim: "answer may overflow", proposed_body: "Use a checked add." };
const duckScenarios = {
  "duck-call": [duckArgs],
  "duck-sequential": [duckArgs, { ...duckArgs, claim: "second claim" }],
  "duck-then-error": [duckArgs],
  "duck-flood": Array.from({ length: 100 }, () => duckArgs),
  "duck-primary-error": [duckArgs],
  "duck-parallel": [duckArgs, { ...duckArgs, claim: "second claim" }],
  "duck-invalid": [{ ...duckArgs, path: "unchanged.rs" }],
};
const duckCalls = duckScenarios[scenario];
let duckResults = 0;
// Sequential scenarios call the next duck only after the previous result.
const sequential = ["duck-sequential"].includes(scenario);
// Duck turns that started and neither ended nor were aborted.
let activeTurns = 0;
const duckCall = (index) => {
  event("tool.execution_start", { toolName: "rubber_duck" });
  event("external_tool.requested", {
    sessionId, requestId: `duck-${index + 1}`, toolCallId: `duck-${index + 1}`,
    toolName: "rubber_duck", arguments: duckCalls[index],
  });
};
const isExit = exitScenarios.includes(scenario);
const longNano = Array.from({ length: 120 }, () => "word").join(" ");
const okReport = {
  findings: [{ file: "source.rs", line: 1, nano: "unwrap may panic", full: "Full detail.", fix: "Use ?." }],
};
const badReport = { findings: [{ ...okReport.findings[0], nano: longNano }] };
const firstExit = {
  "exit-ready": ["ready_for_human", { summary: "No findings." }],
  "exit-comments": ["leave_comments", { new_comments: [{ path: "source.rs", line: 1, body: "Use a constant." }] }],
  "exit-denied-then-ok": ["leave_comments", { new_comments: [{ path: "source.rs", line: 1, body: "Use a constant." }] }],
  "exit-catalog-mismatch": ["ready_for_human", { summary: "No findings." }],
  "exit-approve": ["approve_pull_request", { comment: "Reviewed. Approving." }],
  "exit-merge": ["approve_and_merge", { comment: "Reviewed. Approving and merging." }],
  "exit-approve-denied-self": ["approve_pull_request", { comment: "Reviewed. Approving." }],
  panic: ["panic_exit", { reason: "tool broken", detail: "read_changes failed twice." }],
  "report-ok": ["submit_report", okReport],
  "report-too-long": ["submit_report", badReport],
  "exit-malformed-forever": ["submit_report", badReport],
  "verdict-ok": ["submit_verdict", { verdict: "confirmed", nano: "real overflow", evidence: [{ path: "source.rs", line: 1 }] }],
  "verdict-revise-bad-then-ok": ["submit_verdict", { verdict: "revise", nano: "wrong body" }],
};
let exitCount = 0;
let exitEnded = false;
const toolCall = (name, args) => {
  const id = `exit-${++exitCount}`;
  event("tool.execution_start", { toolName: name });
  event("external_tool.requested", {
    sessionId, requestId: id, toolCallId: id, toolName: name, arguments: args,
  });
};
// Scenarios without their own exit hand over once their reads are done.
const handOver = () => toolCall("ready_for_human", { summary: "Synthetic review done." });
// A successful terminal tool ends the turn with no final assistant message.
const endTurn = () => {
  if (exitEnded) return;
  exitEnded = true;
  activeTurns = Math.max(0, activeTurns - 1);
  event("assistant.usage", { inputTokens: 20, outputTokens: 10 });
  event("session.idle", {});
};
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
        globalThis.toolNames = (request.params.tools ?? []).map((t) => t.name);
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
            ...globalThis.toolNames,
            ...(scenario === "extra-tool" || scenario === "exit-catalog-mismatch"
              ? ["bash"]
              : []),
          ].map((name) => ({ name, description: name })),
        });
        break;
      case "session.send":
        respond(request.id, { messageId: randomUUID() });
        receipt({ method: "fixture.prompt", prompt: request.params.prompt ?? null });
        if (duckCalls) {
          setTimeout(() => {
            if (sequential) duckCall(0);
            else duckCalls.forEach((_, index) => duckCall(index));
            // The primary run fails on its own while the duck call is still pending.
            if (scenario === "duck-primary-error")
              setTimeout(() => event("session.error", { statusCode: 500 }), 300);
          }, 10);
          break;
        }
        if (scenario.startsWith("verdict-")) {
          receipt({ method: "fixture.turns", active: activeTurns });
          activeTurns += 1;
          exitEnded = false;
        }
        if (scenario === "verdict-slow") break;
        if (scenario === "verdict-none") {
          setTimeout(() => {
            event("assistant.usage", { inputTokens: 5, outputTokens: 2 });
            event("session.idle", {});
          }, 10);
          break;
        }
        if (scenario === "verdict-429") {
          setTimeout(() => event("session.error", { statusCode: 429, retryAfterSeconds: 30 }), 10);
          break;
        }
        if (isExit) {
          setTimeout(() => {
            if (scenario === "exit-none") {
              event("assistant.usage", { inputTokens: 20, outputTokens: 10 });
              event("assistant.message", { content: "Done, nothing to exit with." });
              event("session.idle", {});
            } else if (scenario === "exit-double") {
              toolCall("ready_for_human", { summary: "First exit." });
              toolCall("leave_comments", {
                new_comments: [{ path: "source.rs", line: 1, body: "Second exit." }],
              });
            } else {
              toolCall(...firstExit[scenario]);
            }
          }, 10);
          break;
        }
        if (["read-full", "read-source-only", "tool-invalid-path"].includes(scenario)) {
          const toolName = scenario === "read-source-only" ? "read_source" : "read_changes";
          event("tool.execution_start", { toolName });
          event("external_tool.requested", {
            sessionId,
            requestId: "final-read",
            toolCallId: "final-read",
            toolName,
            arguments: scenario === "read-source-only"
              ? { path: "source.rs", side: "head" }
              : { path: scenario === "tool-invalid-path" ? "missing.rs" : "source.rs", part: "before" },
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
          if (scenario.startsWith("malformed")) {
            // No exit: the run ends without a result.
            event("assistant.usage", { inputTokens: 20, outputTokens: 10 });
            event("assistant.message", { content: "not JSON" });
            event("session.idle", {});
            return;
          }
          handOver();
        }, 10);
        break;
      case "session.tools.handlePendingToolCall":
        receipt({ method: "fixture.finalRead", result: request.params.result });
        respond(request.id, {});
        if (String(request.params.requestId).startsWith("duck-")) {
          receipt({ method: "fixture.duck", requestId: request.params.requestId, result: request.params.result, at: Date.now() });
          ++duckResults;
          if (duckResults < duckCalls.length) {
            if (sequential) duckCall(duckResults);
          } else if (scenario === "duck-then-error") {
            event("session.error", { statusCode: 500 });
          } else {
            handOver();
          }
          break;
        }
        if (String(request.params.requestId).startsWith("exit-")) {
          const failed = JSON.stringify(request.params.result ?? null).includes('"failure"');
          receipt({ method: "fixture.exit", requestId: request.params.requestId, failed });
          if (!failed) {
            endTurn();
          } else if (scenario === "exit-approve-denied-self") {
            toolCall("ready_for_human", { summary: "Own PR; handing over." });
          } else if (scenario === "exit-denied-then-ok") {
            toolCall("ready_for_human", { summary: "Comments refused; handing over." });
          } else if (scenario === "report-too-long") {
            toolCall("submit_report", okReport);
          } else if (scenario === "exit-malformed-forever") {
            toolCall("submit_report", badReport);
          } else if (scenario === "verdict-revise-bad-then-ok") {
            toolCall("submit_verdict", { verdict: "revise", nano: "wrong line", revised_body: "Use checked_add on line 1." });
          }
          break;
        }
        if (["read-source-only", "tool-invalid-path"].includes(scenario)) {
          // The tool call (served or rejected) does not decide the review.
          handOver();
          break;
        }
        if (
          scenario === "read-full" &&
          request.params.requestId === "final-read" &&
          JSON.stringify(request.params.result ?? null).includes("answer")
        ) {
          event("external_tool.requested", {
            sessionId,
            requestId: "final-read-after",
            toolCallId: "final-read-after",
            toolName: "read_changes",
            arguments: { path: "source.rs", part: "after" },
          });
          break;
        }
        if (
          scenario !== "read-full" ||
          request.params.requestId !== "final-read-after" ||
          !JSON.stringify(request.params.result ?? null).includes("answer")
        ) {
          event("session.error", { statusCode: 500 });
        } else {
          handOver();
        }
        break;
      case "session.abort":
        receipt({ method: "fixture.aborted", at: Date.now(), active: activeTurns });
        activeTurns = Math.max(0, activeTurns - 1);
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
