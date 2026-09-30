import { isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repository = fileURLToPath(new URL("../../", import.meta.url));
const configuredTarget = process.env.CARGO_TARGET_DIR;
export const target = configuredTarget
  ? isAbsolute(configuredTarget)
    ? configuredTarget
    : resolve(repository, configuredTarget)
  : join(repository, "src-tauri", "target");
export const bridge = join(
  target,
  "debug",
  "examples",
  `settings_bridge${process.platform === "win32" ? ".exe" : ""}`,
);
