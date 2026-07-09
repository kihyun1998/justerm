import { defineConfig } from "@playwright/test";

const PORT = 8269;

export default defineConfig({
  testDir: "./e2e",
  // Only the browser specs. `proof.test.mjs` lives here too but belongs to node's own runner
  // (`pnpm test:unit`), and Playwright's default `testMatch` would otherwise try to collect it.
  testMatch: "**/*.spec.mjs",
  // `context-loss-timeout.html` deliberately waits out two restore deadlines.
  timeout: 60_000,
  fullyParallel: false,
  // A retry would hide the flake it is meant to survive. These proofs read pixels; if they are not
  // deterministic, that is the finding.
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: [["list"]],
  use: { baseURL: `http://127.0.0.1:${PORT}` },
  webServer: {
    command: "node scripts/serve.mjs",
    url: `http://127.0.0.1:${PORT}/demo/index.html`,
    reuseExistingServer: !process.env.CI,
    timeout: 20_000,
  },
});
