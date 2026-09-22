import { defineConfig } from "vite";

export default defineConfig({
  // Keep JS syntax and inherited CSS optimization targets on macOS 12's WebKit.
  build: { target: "safari15", assetsInlineLimit: 0 },
  server: { port: 1420, strictPort: true },
  clearScreen: false,
});
