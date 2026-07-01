import { expect, test } from "@playwright/test";

/**
 * End-to-end verification of the demo's a11y features in a REAL headless browser
 * — the automated form of the F-key/HITL smoke. We can't hear the WebAudio earcon
 * or run a screen reader, but we assert the exact things an SR consumes: the
 * aria-live region's text (#160 announce) and the signal path (via its console
 * log), plus the #161 gate that suppresses both. The real wasm decoder + the real
 * controllers run behind the demo's stub backend.
 */

const live = "[data-testid='command-live']";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  // The control bar mounts synchronously; wait for it to prove the app booted.
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible();
});

test("control bar shows the four action buttons", async ({ page }) => {
  await expect(page.getByRole("button", { name: /Accessible view/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Alt screen: (ON|OFF)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Screen reader: (ON|OFF)/ })).toBeVisible();
});

test("alt screen button toggles its label", async ({ page }) => {
  await page.getByRole("button", { name: "Alt screen: OFF" }).click();
  await expect(page.getByRole("button", { name: "Alt screen: ON" })).toBeVisible();
  await page.getByRole("button", { name: "Alt screen: ON" }).click();
  await expect(page.getByRole("button", { name: "Alt screen: OFF" })).toBeVisible();
});

test("finish command announces success then failure to the live region", async ({ page }) => {
  const signals: string[] = [];
  page.on("console", (msg) => {
    const t = msg.text();
    if (t.includes("[demo] signal:")) signals.push(t);
  });

  // First finish → exit 0 → success announce + success signal.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command succeeded");

  // Second finish → exit 1 → failure announce (with the code) + failure signal.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command failed, exit 1");

  expect(signals.some((s) => s.includes("succeeded"))).toBe(true);
  expect(signals.some((s) => s.includes("failed"))).toBe(true);
});

test("screen-reader-off suppresses the announce; back on resumes it (#161)", async ({ page }) => {
  // Turn SR off — the host telling justerm no screen reader is present.
  await page.getByRole("button", { name: "Screen reader: ON" }).click();
  await expect(page.getByRole("button", { name: "Screen reader: OFF" })).toBeVisible();

  // A finished command must NOT reach the live region while SR is inactive.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("");

  // Turn SR back on — announces resume.
  await page.getByRole("button", { name: "Screen reader: OFF" }).click();
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).not.toHaveText("");
});

test("accessible view opens as a document overlay and Escape closes it", async ({ page }) => {
  const doc = page.locator("[role='document']");
  await expect(doc).toBeHidden();

  await page.getByRole("button", { name: /Accessible view/ }).click();
  await expect(doc).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(doc).toBeHidden();
});
