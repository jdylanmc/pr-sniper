import { build } from "esbuild";
import fs from "node:fs/promises";
import path from "node:path";
import { builtinModules } from "node:module";

const pkg = JSON.parse(await fs.readFile("package.json", "utf8"));
const output = "src-tauri/resources/mcp/mcp-server.mjs";
const check = process.argv.includes("--check");
const result = await build({
  entryPoints: ["src-control/mcp-server.mjs"],
  outfile: output,
  bundle: true,
  minify: true,
  platform: "node",
  format: "esm",
  target: "node22",
  legalComments: "external",
  metafile: true,
  write: false,
  define: { PR_SNIPER_VERSION: JSON.stringify(pkg.version) },
  banner: {
    js: 'import { createRequire as __createRequire } from "node:module"; const require = __createRequire(import.meta.url);',
  },
});
async function emit(file, bytes) {
  if (check) {
    if (!Buffer.from(bytes).equals(await fs.readFile(file)))
      throw new Error(
        `Stale bundled MCP asset: ${file}; run npm run build:control-mcp`,
      );
  } else await fs.writeFile(file, bytes);
}
const builtins = new Set([
  ...builtinModules,
  ...builtinModules.map((name) => `node:${name}`),
]);
for (const file of Object.values(result.metafile.outputs)) {
  for (const dependency of file.imports) {
    if (dependency.external && !builtins.has(dependency.path))
      throw new Error(`Unbundled runtime dependency: ${dependency.path}`);
  }
}
const bundle = result.outputFiles.find(
  (file) =>
    file.path.endsWith("/mcp-server.mjs") ||
    file.path.endsWith("\\mcp-server.mjs"),
);
if (!bundle || bundle.contents.length > 1024 * 1024)
  throw new Error("MCP bundle exceeds native package bound");
const packages = new Map();
for (const input of Object.keys(result.metafile.inputs).filter((name) =>
  name.includes("node_modules/"),
)) {
  let directory = path.dirname(input);
  while (directory.includes("node_modules")) {
    try {
      const metadata = JSON.parse(
        await fs.readFile(path.join(directory, "package.json"), "utf8"),
      );
      if (metadata.name) {
        packages.set(metadata.name, { directory, version: metadata.version });
        break;
      }
    } catch {}
    directory = path.dirname(directory);
  }
}
const notices = [];
for (const [name, { directory, version }] of [...packages].sort()) {
  let license;
  for (const file of ["LICENSE", "LICENSE.md", "LICENSE.txt"]) {
    try {
      license = await fs.readFile(path.join(directory, file), "utf8");
      break;
    } catch {}
  }
  if (!license) throw new Error(`Missing license for bundled ${name}`);
  notices.push(`${name} ${version}\n${license.trim()}`);
}
for (const file of result.outputFiles) await emit(file.path, file.contents);
await emit(
  "src-tauri/resources/mcp/THIRD_PARTY_NOTICES.txt",
  Buffer.from(notices.join("\n\n---\n\n") + "\n"),
);
console.log(
  `Bundled MCP server (${(await fs.stat(output)).size} bytes; ${packages.size} licensed packages).`,
);
