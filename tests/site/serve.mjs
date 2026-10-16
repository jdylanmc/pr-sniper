// Serves the GitHub Pages artifact layout: site/ at the root, CHANGELOG.md, and
// the v2 design prototype under /demo/.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const site = join(root, "site");
const demo = join(root, "prototypes", "v2");
// Same files pages.yml copies into demo/; tests and evidence stay out.
const demoFiles = new Set([
  "/index.html",
  "/style.css",
  "/app.js",
  "/model.js",
  "/sniper-mark.png",
  "/wallpaper.svg",
]);
const port = Number(process.env.SITE_TEST_PORT ?? "1431");
const types = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
  ".md": "text/markdown; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".png": "image/png",
};

createServer(async (request, response) => {
  const path = decodeURIComponent(new URL(request.url, "http://x").pathname);
  const relative = normalize(path === "/" ? "/index.html" : path).replace(
    /^(\.\.[/\\])+/,
    "",
  );
  const isDemo = relative === "/demo" || relative.startsWith("/demo/");
  const demoPath = relative.slice("/demo".length) || "/";
  const file =
    relative === "/CHANGELOG.md"
      ? join(root, "CHANGELOG.md")
      : isDemo
        ? join(demo, demoPath === "/" ? "index.html" : demoPath)
        : join(site, relative);
  if (
    (isDemo && !demoFiles.has(demoPath === "/" ? "/index.html" : demoPath)) ||
    (relative !== "/CHANGELOG.md" && !file.startsWith(isDemo ? demo : site))
  ) {
    response.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(file);
    response.writeHead(200, {
      "content-type": types[extname(file)] ?? "application/octet-stream",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`site on http://127.0.0.1:${port}/`);
});
