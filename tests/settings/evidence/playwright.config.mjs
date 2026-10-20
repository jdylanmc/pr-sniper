import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "*.fixture.mjs",
  retries: 0,
  workers: 1,
  reporter: "line",
  use: { viewport: { width: 40, height: 30 } },
});
