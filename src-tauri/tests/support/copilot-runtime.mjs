// Synthetic SDK transport, never an authentication or Copilot service substitute.
import { appendFileSync } from "node:fs";

const args = process.argv.slice(2);
const tokenIndex = args.indexOf("--auth-token-env");
const selected = process.env[args[tokenIndex + 1]];
const allowed = new Set(["account-a", "account-b", "blocked", "waiting"]);
const receipt = (value) =>
  appendFileSync(process.env.TEST_RECEIPT, `${JSON.stringify(value)}\n`);
receipt({
  pid: process.pid,
  args,
  home: process.env.HOME,
  copilotHome: process.env.COPILOT_HOME,
  keytarDisabled: process.env.COPILOT_DISABLE_KEYTAR,
  ambient: [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "COPILOT_GITHUB_TOKEN",
    "ANTHROPIC_API_KEY",
    "NODE_OPTIONS",
  ].filter((key) => process.env[key]),
});
const respond = (id, result, error) => {
  const body = JSON.stringify({
    jsonrpc: "2.0",
    id,
    ...(error ? { error } : { result }),
  });
  process.stdout.write(
    `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`,
  );
};
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
    receipt({ method: request.method });
    if (request.id === undefined) continue;
    switch (request.method) {
      case "connect":
        if (selected === "waiting-start") break;
        if (selected === "bad-start") {
          respond(request.id, null, {
            code: -32600,
            message: "Synthetic startup failure",
          });
          break;
        }
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
          isAuthenticated: allowed.has(selected),
          authType: "token",
          login: selected,
        });
        break;
      case "models.list":
        if (selected === "waiting") break;
        if (selected === "blocked")
          respond(request.id, null, {
            code: -32001,
            message: "secret-provider-error-must-not-escape",
          });
        else
          respond(request.id, {
            models: [
              {
                id: `model-${selected}`,
                name: `Model ${selected}`,
                capabilities: {},
              },
            ],
          });
        break;
      case "runtime.shutdown":
        respond(request.id, {});
        break;
      default:
        respond(request.id, null, {
          code: -32601,
          message: "Forbidden method in catalog-only fixture",
        });
    }
  }
});
