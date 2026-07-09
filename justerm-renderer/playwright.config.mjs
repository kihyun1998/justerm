import { defineConfig } from "@playwright/test";

const PORT = 8269;

export default defineConfig({
  testDir: "./e2e",
  // `context-loss-timeout.html` deliberately waits out two restore deadlines.
  timeout: 60_000,
  fullyParallel: false,
  reporter: [["list"]],
  use: { baseURL: `http://127.0.0.1:${PORT}` },
  webServer: {
    command: "node scripts/serve.mjs",
    url: `http://127.0.0.1:${PORT}/demo/index.html`,
    reuseExistingServer: !process.env.CI,
    timeout: 20_000,
  },
});
