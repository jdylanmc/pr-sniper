import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import net from "node:net";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";

export const protocol = 1;
const maxFrame = 1024 * 1024;
const magic = Buffer.from("SNIPER1\n");
const directory = path.dirname(fileURLToPath(import.meta.url));

function failure(code, detail = null) {
  return Object.assign(new Error(code), { code, detail });
}

export function dataRoot(
  env = process.env,
  platform = process.platform,
  home = os.homedir(),
) {
  const paths = platform === "win32" ? path.win32 : path.posix;
  const absolute = (value) =>
    paths.isAbsolute(value) &&
    (platform !== "win32" ||
      /^(?:[a-z]:[\\/]|[\\/]{2}[^\\/]+[\\/][^\\/]+)/i.test(value));
  if (env.PR_SNIPER_DATA_DIR) {
    if (!absolute(env.PR_SNIPER_DATA_DIR))
      throw failure("invalid_data_directory");
    return env.PR_SNIPER_DATA_DIR;
  }
  if (platform === "darwin")
    return paths.join(
      home,
      "Library",
      "Application Support",
      "com.jdylanmc.pr-sniper",
    );
  if (platform === "win32" && env.LOCALAPPDATA) {
    if (!absolute(env.LOCALAPPDATA)) throw failure("invalid_data_directory");
    return paths.join(env.LOCALAPPDATA, "com.jdylanmc.pr-sniper");
  }
  throw failure("unsupported_platform");
}

async function endpoint(root) {
  const control = path.join(root, "control");
  const dir = await fs.lstat(control);
  const file = path.join(control, "endpoint.json");
  const stat = await fs.lstat(file);
  if (
    !dir.isDirectory() ||
    dir.isSymbolicLink() ||
    !stat.isFile() ||
    stat.isSymbolicLink() ||
    stat.size > 4096
  )
    throw failure("unsafe_endpoint");
  if (
    process.platform !== "win32" &&
    (dir.mode & 0o077 ||
      stat.mode & 0o077 ||
      dir.uid !== process.getuid() ||
      stat.uid !== process.getuid())
  )
    throw failure("unsafe_endpoint");
  const value = JSON.parse(await fs.readFile(file, "utf8"));
  if (value.protocol !== protocol) throw failure("incompatible_protocol");
  if (
    !Number.isInteger(value.port) ||
    value.port < 1 ||
    value.port > 65535 ||
    !/^[a-f0-9]{64}$/.test(value.token)
  )
    throw failure("invalid_endpoint");
  return value;
}

export function exchange(address, request, timeout = 120_000) {
  const payload = Buffer.from(
    JSON.stringify({ protocol, token: address.token, request }),
  );
  if (payload.length > maxFrame)
    return Promise.reject(failure("request_too_large"));
  return new Promise((resolve, reject) => {
    const client = net.createConnection({
      host: "127.0.0.1",
      port: address.port,
    });
    let received = Buffer.alloc(0);
    let sent = false;
    const timer = setTimeout(() => {
      client.destroy();
      reject(
        failure(sent ? "outcome_unknown" : "unavailable", {
          request_id: request.request_id,
        }),
      );
    }, timeout);
    const done = (error, value) => {
      clearTimeout(timer);
      client.destroy();
      if (error) reject(error);
      else resolve(value);
    };
    client.on("connect", () => {
      const header = Buffer.alloc(12);
      magic.copy(header);
      header.writeUInt32BE(payload.length, 8);
      sent = true;
      client.write(Buffer.concat([header, payload]));
    });
    client.on("data", (bytes) => {
      if (received.length + bytes.length > maxFrame + 4)
        return done(failure("response_too_large"));
      received = Buffer.concat([received, bytes]);
      if (received.length < 4) return;
      const length = received.readUInt32BE(0);
      if (length > maxFrame) return done(failure("response_too_large"));
      if (received.length === length + 4) {
        try {
          const reply = JSON.parse(received.subarray(4).toString("utf8"));
          if (reply.protocol !== protocol || typeof reply.ok !== "boolean")
            throw failure("invalid_response");
          done(null, reply);
        } catch {
          done(failure("invalid_response"));
        }
      }
    });
    client.on("error", () =>
      done(
        failure(sent ? "outcome_unknown" : "unavailable", {
          request_id: request.request_id,
        }),
      ),
    );
    client.on("end", () => {
      if (
        received.length < 4 ||
        received.length !== received.readUInt32BE(0) + 4
      )
        done(failure("outcome_unknown", { request_id: request.request_id }));
    });
  });
}

async function executable() {
  try {
    const file = path.join(directory, "..", "launcher.json");
    const stat = await fs.lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 16384)
      throw failure("unsafe_launcher");
    const launch = JSON.parse(await fs.readFile(file, "utf8"));
    if (launch.protocol !== protocol || !path.isAbsolute(launch.executable))
      throw failure("incompatible_launcher");
    return launch.executable;
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  const candidates =
    process.platform === "darwin"
      ? [
          "/Applications/PR Sniper.app/Contents/MacOS/pr-sniper",
          path.join(
            os.homedir(),
            "Applications/PR Sniper.app/Contents/MacOS/pr-sniper",
          ),
        ]
      : process.platform === "win32" && process.env.LOCALAPPDATA
        ? [path.join(process.env.LOCALAPPDATA, "PR Sniper", "pr-sniper.exe")]
        : [];
  for (const file of candidates) {
    try {
      if ((await fs.stat(file)).isFile()) return file;
    } catch {
      /* next installed location */
    }
  }
  throw failure("app_not_installed");
}

async function launch() {
  if (process.env.PR_SNIPER_DATA_DIR && !process.env.PR_SNIPER_KEYCHAIN_SERVICE)
    throw failure("isolated_keychain_required");
  const child = spawn(await executable(), ["--background"], {
    detached: true,
    stdio: "ignore",
    windowsHide: true,
  });
  await new Promise((resolve, reject) => {
    child.once("spawn", resolve);
    child.once("error", () => reject(failure("launch_failed")));
  });
  child.unref();
}

export async function invoke(request) {
  const root = dataRoot();
  request = {
    ...request,
    request_id: request.request_id ?? randomUUID(),
    issued_at: request.issued_at ?? Math.floor(Date.now() / 1000),
  };
  const receipt = {
    request_id: request.request_id,
    issued_at: request.issued_at,
  };
  const send = async () => {
    try {
      const response = await exchange(await endpoint(root), request);
      return { ...response, request: receipt };
    } catch (error) {
      if (error.code === "outcome_unknown")
        error.detail = { ...error.detail, ...receipt };
      throw error;
    }
  };
  try {
    return await send();
  } catch (error) {
    if (!["ENOENT", "unavailable"].includes(error.code)) throw error;
  }
  await launch();
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    try {
      return await send();
    } catch (error) {
      if (!["ENOENT", "unavailable"].includes(error.code)) throw error;
    }
  }
  throw failure("app_not_ready");
}
