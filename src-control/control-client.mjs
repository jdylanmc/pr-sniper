import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import net from "node:net";
import { spawn } from "node:child_process";
import {
  createHash,
  createHmac,
  randomBytes,
  randomUUID,
  timingSafeEqual,
} from "node:crypto";

export const protocol = 2;
const maxFrame = 1024 * 1024;
const magic = Buffer.from("SNIPER1\n");
const hex64 = /^[a-f0-9]{64}$/;
const handshakeTimeout = 5_000;
// Domain-separated labels: no proof is valid in another role.
const labels = {
  challenge: "pr-sniper/control/2/challenge",
  request: "pr-sniper/control/2/request",
  response: "pr-sniper/control/2/response",
};

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
  if (value.protocol !== protocol) {
    // A crashed app of another version leaves a descriptor with no listener.
    if (Number.isInteger(value.pid) && value.pid > 0 && !alive(value.pid))
      throw failure("unavailable");
    throw failure("incompatible_protocol");
  }
  if (
    !Number.isInteger(value.port) ||
    value.port < 1 ||
    value.port > 65535 ||
    !Number.isInteger(value.pid) ||
    value.pid < 1 ||
    !hex64.test(value.token)
  )
    throw failure("invalid_endpoint");
  // A crashed app leaves its descriptor behind; never contact its old port.
  if (!alive(value.pid)) throw failure("unavailable");
  return value;
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/**
 * HMAC-SHA256 over `len ‖ label` then each `len ‖ field` (u32 big-endian
 * lengths). Fields: nonces (hex text), then request/response SHA-256 digests.
 */
export function mac(token, role, ...fields) {
  const hmac = createHmac("sha256", token);
  for (const part of [labels[role], ...fields].map((v) => Buffer.from(v))) {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(part.length);
    hmac.update(length).update(part);
  }
  return hmac.digest("hex");
}

export const sha256 = (bytes) => createHash("sha256").update(bytes).digest();

export function proofMatches(actual, expected) {
  return (
    typeof actual === "string" &&
    hex64.test(actual) &&
    hex64.test(expected) &&
    timingSafeEqual(Buffer.from(actual, "hex"), Buffer.from(expected, "hex"))
  );
}

function frame(value) {
  const payload = Buffer.from(JSON.stringify(value));
  const header = Buffer.alloc(12);
  magic.copy(header);
  header.writeUInt32BE(payload.length, 8);
  return Buffer.concat([header, payload]);
}

/**
 * Mutually authenticated exchange. The listener must first prove possession
 * of the capability for a fresh client nonce; until it does, the client has
 * sent only that nonce. The raw capability never crosses the socket. The
 * request proof covers the exact request bytes sent, and the response proof
 * covers those and the exact response bytes, so a relay can alter neither.
 */
export function exchange(address, request, timeout = 120_000) {
  const nonce = randomBytes(32).toString("hex");
  const hello = frame({ protocol, nonce });
  const raw = Buffer.from(JSON.stringify(request));
  const digest = sha256(raw);
  const body = (proof) =>
    frame({ protocol, proof, request: raw.toString("base64") });
  if (body("0".repeat(64)).length - 12 > maxFrame)
    return Promise.reject(failure("request_too_large"));
  return new Promise((resolve, reject) => {
    const client = net.createConnection({
      host: "127.0.0.1",
      port: address.port,
    });
    let received = Buffer.alloc(0);
    let server;
    let sent = false;
    const lost = () =>
      failure(sent ? "outcome_unknown" : "unavailable", {
        request_id: request.request_id,
      });
    const expire = (ms) =>
      setTimeout(() => {
        client.destroy();
        reject(lost());
      }, ms);
    let timer = expire(Math.min(timeout, handshakeTimeout));
    const done = (error, value) => {
      clearTimeout(timer);
      client.destroy();
      if (error) reject(error);
      else resolve(value);
    };
    const exact = (value, keys) =>
      value !== null &&
      typeof value === "object" &&
      Object.keys(value).sort().join() === keys &&
      value.protocol === protocol;
    client.on("connect", () => client.write(hello));
    const reply = (bytes) => {
      const value = JSON.parse(bytes.toString("utf8"));
      if (!sent) {
        if (
          !exact(value, "nonce,proof,protocol") ||
          !hex64.test(value.nonce) ||
          !proofMatches(
            value.proof,
            mac(address.token, "challenge", nonce, value.nonce),
          )
        )
          throw new Error();
        server = value.nonce;
        clearTimeout(timer);
        timer = expire(timeout);
        sent = true;
        client.write(
          body(mac(address.token, "request", nonce, server, digest)),
        );
        return;
      }
      if (!exact(value, "proof,protocol,response")) throw new Error();
      const response = Buffer.from(value.response, "base64");
      if (
        response.toString("base64") !== value.response ||
        !proofMatches(
          value.proof,
          mac(
            address.token,
            "response",
            nonce,
            server,
            digest,
            sha256(response),
          ),
        )
      )
        throw new Error();
      const result = JSON.parse(response.toString("utf8"));
      if (result?.protocol !== protocol || typeof result.ok !== "boolean")
        throw new Error();
      done(null, result);
    };
    client.on("data", (bytes) => {
      received = Buffer.concat([received, bytes]);
      while (received.length >= 4) {
        const length = received.readUInt32BE(0);
        if (length > maxFrame)
          return done(failure(sent ? "response_too_large" : "unavailable"));
        if (received.length < length + 4) return;
        const next = received.subarray(4, length + 4);
        received = received.subarray(length + 4);
        try {
          reply(next);
        } catch {
          // Before the app's proof: not the app. After sending: the request
          // may have run, but no reply bytes are trusted without the proof.
          return done(lost());
        }
      }
    });
    client.on("error", () => done(lost()));
    client.on("end", () => done(lost()));
  });
}

let configured;

/** The app registers its own absolute executable; never read it from a package. */
export function useExecutable(file) {
  if (typeof file !== "string" || !path.isAbsolute(file))
    throw failure("invalid_executable");
  configured = file;
}

async function executable() {
  if (configured) return configured;
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
