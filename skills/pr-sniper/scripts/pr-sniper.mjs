#!/usr/bin/env node
// Internal diagnostic/acceptance client. Agents use the packaged MCP tools.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { protocol, invoke } from "./control-client.mjs";
export { protocol, invoke, dataRoot, exchange } from "./control-client.mjs";

async function main() {
  if (process.argv[2] === "--version") {
    console.log(JSON.stringify({ protocol, skill: "pr-sniper" }));
    return;
  }
  let input = Buffer.alloc(0);
  if (!process.argv[2])
    for await (const chunk of process.stdin) {
      input = Buffer.concat([input, chunk]);
      if (input.length > 1024 * 1024)
        throw Object.assign(new Error(), { code: "request_too_large" });
    }
  if (process.argv[2] && !["describe", "inspect"].includes(process.argv[2]))
    throw Object.assign(new Error(), { code: "invalid_arguments" });
  const request = process.argv[2]
    ? { operation: process.argv[2] }
    : JSON.parse(input.toString("utf8"));
  const reply = await invoke(request);
  console.log(JSON.stringify(reply));
  process.exitCode = reply.ok ? 0 : 1;
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main().catch((error) => {
    console.log(
      JSON.stringify({
        protocol,
        ok: false,
        error: {
          code: error.code ?? "invalid_request",
          detail: error.detail ?? null,
        },
      }),
    );
    process.exitCode = 2;
  });
}
