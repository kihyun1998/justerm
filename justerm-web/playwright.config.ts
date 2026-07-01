import { defineConfig, devices } from "@playwright/test";

/**
 * E2E for the demo (#160/#161 a11y): drive the real widget in a headless browser
 * and assert the aria-live announce + signal paths a screen reader would consume.
 * The `webServer` runs the actual demo (`pnpm demo` = vite over `demo/`), so the
 * real wasm decoder + controllers run — not a fixture.
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:5173",
    trace: "on-first-retry",
  },
  webServer: {
    command: "pnpm demo",
    url: "http://localhost:5173",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        // Let the WebAudio earcon start without a prior gesture (the click is a
        // gesture anyway; this just avoids console noise in headless).
        launchOptions: { args: ["--autoplay-policy=no-user-gesture-required"] },
      },
    },
  ],
});
