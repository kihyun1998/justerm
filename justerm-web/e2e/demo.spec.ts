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

test("control bar shows the action buttons", async ({ page }) => {
  await expect(page.getByRole("button", { name: /Accessible view/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Alt screen: (ON|OFF)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Announce: (TERSE|VERBOSE)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Screen reader: (ON|OFF)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Prev command" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Next command" })).toBeVisible();
});

test("alt screen button toggles its label", async ({ page }) => {
  await page.getByRole("button", { name: "Alt screen: OFF" }).click();
  await expect(page.getByRole("button", { name: "Alt screen: ON" })).toBeVisible();
  await page.getByRole("button", { name: "Alt screen: ON" }).click();
  await expect(page.getByRole("button", { name: "Alt screen: OFF" })).toBeVisible();
});

// #189: an alt-scoped decoration (created on the alt screen) is DISPOSED on
// alt-leave — core fires MarkerDisposed on ?1049l (per-buffer clearAllMarkers), which
// the demo forwards to `decorations.onMarkerDisposed`. The green highlight is a
// beamterm canvas paint (not DOM, so headless can't see the pixel), but the disposal
// is observable via the Decorate toggle returning to OFF (the handle is gone, not
// merely off-screen) plus the demo's dispose log. A primary decoration, by contrast,
// survives an alt round-trip (only alt-scoped markers dispose) — locking "no
// cross-buffer teardown". This complements the live-screenshot proof so the DOM-
// observable half of the lifecycle is a regression gate, not a one-time eyeball.
test("alt-scoped decoration disposes on alt-leave; a primary decoration survives (#189)", async ({
  page,
}) => {
  const disposeLogs: string[] = [];
  page.on("console", (msg) => {
    if (msg.text().includes("alt-leave disposed the alt-scoped decoration")) {
      disposeLogs.push(msg.text());
    }
  });

  // Alt-scoped: decorate on the alt screen, then leave → the toggle flips back to OFF.
  await page.getByRole("button", { name: "Alt screen: OFF" }).click(); // enter alt
  await page.getByRole("button", { name: "Decorate line: OFF" }).click(); // decorate (alt-scoped)
  await expect(page.getByRole("button", { name: "Decorate line: ON" })).toBeVisible();

  await page.getByRole("button", { name: "Alt screen: ON" }).click(); // leave alt → dispose
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();
  expect(disposeLogs).toHaveLength(1);

  // Primary: decorate on the primary screen, round-trip through alt → still ON, and no
  // further dispose (the alt-leave teardown is alt-scoped only — primary untouched).
  await page.getByRole("button", { name: "Decorate line: OFF" }).click(); // decorate (primary)
  await expect(page.getByRole("button", { name: "Decorate line: ON" })).toBeVisible();
  await page.getByRole("button", { name: "Alt screen: OFF" }).click(); // enter alt
  await page.getByRole("button", { name: "Alt screen: ON" }).click(); // leave alt
  await expect(page.getByRole("button", { name: "Decorate line: ON" })).toBeVisible();
  expect(disposeLogs).toHaveLength(1);
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

test("terse announce drops the exit code on failure (#179)", async ({ page }) => {
  // Flip the announce text to terse (VSCode parity — the exit code is not spoken).
  await page.getByRole("button", { name: "Announce: VERBOSE" }).click();
  await expect(page.getByRole("button", { name: "Announce: TERSE" })).toBeVisible();

  // First finish → exit 0 → success text is identical in either mode.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command succeeded");

  // Second finish → exit 1 → terse omits the code ("Command failed", NOT
  // "Command failed, exit 1"). Proves the injected preset flows through the real
  // controller + aria-live path end-to-end, not just the unit fake.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command failed");
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

test("command nav walks history: announces the command + fires its signal (#166)", async ({
  page,
}) => {
  const signals: string[] = [];
  page.on("console", (msg) => {
    const t = msg.text();
    if (t.includes("[demo] signal:")) signals.push(t);
  });

  // The 0-based index of the focused line within the accessible-view document —
  // the DOM side-effect of reveal() (announce/signal alone would NOT prove the
  // reading cursor moved). The demo's stub commands sit at document lines 0/2/4.
  const focusedLine = () =>
    page.evaluate(() => {
      const doc = document.querySelector("[role='document']");
      return doc ? Array.prototype.indexOf.call(doc.children, document.activeElement) : -1;
    });

  // Open the accessible view so nav loads the command list (cursor at the end).
  await page.getByRole("button", { name: /Accessible view/ }).click();
  await expect(page.locator("[role='document']")).toBeVisible();

  // Prev from the end → last preset command ("ls -la", exit 0): announced on the
  // polite region, a success signal, AND focus revealed on its document line (4).
  // This is the real CommandNavController + DomAccessibleView + wasm.
  await page.getByRole("button", { name: "Prev command" }).click();
  await expect(page.locator(live)).toHaveText("ls -la");
  expect(await focusedLine()).toBe(4); // reveal() moved focus to the command line

  // Prev again → the failing command ("false", exit 1): announce + fail signal +
  // focus revealed on line 2.
  await page.getByRole("button", { name: "Prev command" }).click();
  await expect(page.locator(live)).toHaveText("false");
  expect(await focusedLine()).toBe(2);

  // Next → forward to "ls -la" again (VSCode Next = line > cursor, nearest).
  await page.getByRole("button", { name: "Next command" }).click();
  await expect(page.locator(live)).toHaveText("ls -la");
  expect(await focusedLine()).toBe(4);

  expect(signals.some((s) => s.includes("succeeded"))).toBe(true);
  expect(signals.some((s) => s.includes("failed"))).toBe(true);
});

test("row-tree churn is skipped while SR inactive, re-syncs on reactivation (#169)", async ({
  page,
}) => {
  // The hidden review row-tree (role=list) mirrors the viewport. Its concatenated
  // row text is the DOM-state proxy for "did the tree churn this frame".
  const treeText = () =>
    page.evaluate(() => {
      const list = document.querySelector("[role='list']");
      return list ? Array.from(list.children, (c) => c.textContent).join("|") : null;
    });

  // SR ON (default): as output appends every 300ms the tree tracks the changing
  // viewport — so its text differs after a few frames.
  const before = await treeText();
  await page.waitForTimeout(900);
  expect(await treeText()).not.toBe(before);

  // Turn SR OFF → the per-frame setRow churn is skipped: the tree FREEZES even
  // though output keeps flowing (the win — no DOM work nobody reads).
  await page.getByRole("button", { name: "Screen reader: ON" }).click();
  const frozen = await treeText();
  await page.waitForTimeout(900); // several frames append while inactive
  expect(await treeText()).toBe(frozen); // no churn — unchanged

  // Turn SR ON → syncTree re-renders from the cached latest frame at once (no
  // cold rebuild, no waiting for the next frame): the tree is current again.
  await page.getByRole("button", { name: "Screen reader: OFF" }).click();
  expect(await treeText()).not.toBe(frozen);
});

test("accessible view opens as a document overlay and Escape closes it", async ({ page }) => {
  const doc = page.locator("[role='document']");
  await expect(doc).toBeHidden();

  await page.getByRole("button", { name: /Accessible view/ }).click();
  await expect(doc).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(doc).toBeHidden();
});
