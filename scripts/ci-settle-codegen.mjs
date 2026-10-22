// Tauri's generate_context! writes content-addressed assets into the build
// script's OUT_DIR while rustc compiles the crate. Cargo then sees them as newer
// than that compilation and rebuilds the crate on the next invocation. Backdate
// only files written after the build script finished to its output timestamp.
import { existsSync, readdirSync, statSync, utimesSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repository = fileURLToPath(new URL("../", import.meta.url));
const configured = process.env.CARGO_TARGET_DIR;
const target = configured
  ? isAbsolute(configured)
    ? configured
    : resolve(repository, configured)
  : join(repository, "src-tauri", "target");

let settled = 0;
let runs = 0;
for (const profile of ["debug", "release"]) {
  const builds = join(target, profile, "build");
  if (!existsSync(builds)) continue;
  for (const name of readdirSync(builds)) {
    if (!name.startsWith("pr-sniper-")) continue;
    const output = join(builds, name, "output");
    const out = join(builds, name, "out");
    if (!existsSync(output) || !existsSync(out)) continue;
    runs += 1;
    const finished = statSync(output).mtime;
    for (const entry of readdirSync(out, { withFileTypes: true })) {
      if (!entry.isFile()) continue;
      const path = join(out, entry.name);
      const stat = statSync(path);
      if (stat.mtime > finished) {
        utimesSync(path, stat.atime, finished);
        settled += 1;
      }
    }
  }
}
if (runs === 0) {
  console.error(`No pr-sniper build script output under ${target}.`);
  process.exit(1);
}
console.log(
  `Settled ${settled} generated file(s) in ${runs} build script run(s).`,
);
