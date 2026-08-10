import { defineConfig, devices } from "@playwright/test";

/**
 * E2E for the demo (#160/#161 a11y): drive the real widget in a headless browser
 * and assert the aria-live announce + signal paths a screen reader would consume.
 * The `webServer` runs the actual demo (`pnpm demo` = vite over `demo/`), so the
 * real wasm decoder + controllers run — not a fixture.
 */

/**
 * Where the demo is served — one definition for `use.baseURL`, for `webServer`'s
 * health check, and for the #735 warm-up in `e2e/demo.spec.ts`, which navigates
 * from a context it builds itself off the `browser` fixture. Such a context does
 * **not** inherit `use.baseURL` (that is a test-scoped option and `beforeAll` has
 * no access to test-scoped fixtures), so the warm-up needs the literal, and a
 * third hand-written copy of it is a copy that can disagree.
 */
export const DEMO_URL = "http://localhost:5173";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  // Kept at zero on purpose (#735): a retry re-runs against a browser process
  // whose code cache is now warm, so it would always pass — which does not fix a
  // slow boot, it disables the detector. It would also have masked #653.
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: DEMO_URL,
    trace: "on-first-retry",
  },
  webServer: {
    command: "pnpm demo",
    url: DEMO_URL,
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
