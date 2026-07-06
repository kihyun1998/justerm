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

// #217: a native Select-All puts the selection anchor/focus OUTSIDE the hidden row tree
// (on document.body, an ancestor spanning it). The bridge must CLAMP those endpoints to
// the tree instead of no-oping — begin at row 0, col 0 and extend to a later row. Proven
// in a real browser: the demo's a11y `selectionPort` logs `[a11y-sel] begin/extend`, so a
// clamp that fired (vs a silent no-op) is observable via the console signal. This exercises
// the real DOM glue (`compareDocumentPosition` classification + range intersection), which
// the DOM-less unit tests can't.
test("Select-All clamps the out-of-tree selection to the row tree (#217)", async ({ page }) => {
  const selLog: string[] = [];
  page.on("console", (m) => {
    const t = m.text();
    if (t.includes("[a11y-sel]")) selLog.push(t);
  });

  // The row tree mirrors the viewport; wait for it to hold content rows.
  await expect(page.locator("[role='listitem']").first()).toBeAttached();

  // Native Select-All: select everything under <body>, spanning the whole tree.
  await page.evaluate(() => {
    const s = window.getSelection();
    if (!s) throw new Error("no selection");
    s.removeAllRanges();
    s.selectAllChildren(document.body);
  });

  // The clamp fired: begin at the tree start (row 0, col 0) and an extend to a later row —
  // NOT a no-op (pre-#217 this whole selection was dropped because both endpoints were
  // outside the tree).
  await expect.poll(() => selLog.join("\n")).toContain("[a11y-sel] begin 0,0");
  expect(selLog.some((l) => l.includes("[a11y-sel] extend"))).toBe(true);
});

// #217 (Lens-1 edge): an ASYMMETRIC selection — one endpoint resolved inside a row, the
// other a spanning ancestor (e.g. `documentElement`, how some ATs report a "select to
// end"). The out-of-tree end classifies as null (an ancestor contains the whole tree), so
// the rescue must fire on EITHER endpoint being null — not just both — else the whole
// selection is silently dropped. Proven live: the clamp must still emit a begin/extend.
test("asymmetric spanning selection (row → documentElement) still clamps (#217)", async ({
  page,
}) => {
  const selLog: string[] = [];
  page.on("console", (m) => {
    const t = m.text();
    if (t.includes("[a11y-sel]")) selLog.push(t);
  });

  await expect(page.locator("[role='listitem']").first()).toBeAttached();

  await page.evaluate(() => {
    const firstRow = document.querySelector("[role='list'] [role='listitem']");
    const textNode = firstRow?.firstChild;
    if (!textNode) throw new Error("no row text node");
    const r = document.createRange();
    r.setStart(textNode, 0); // anchor INSIDE row 0
    r.setEnd(document.documentElement, document.documentElement.childNodes.length); // focus on a spanning ancestor
    const s = window.getSelection();
    if (!s) throw new Error("no selection");
    s.removeAllRanges();
    s.addRange(r);
  });

  // Not dropped: the spanning-ancestor end clamped, so a real selection was driven.
  await expect.poll(() => selLog.some((l) => l.includes("[a11y-sel] begin"))).toBe(true);
  expect(selLog.some((l) => l.includes("[a11y-sel] extend"))).toBe(true);
});

// #133 (S16): the widget wires input + wheel + focus. Headless can't see the beamterm
// caret paint, but every routing DECISION has a DOM/console proxy the demo exposes: the
// input sink logs intents (`[input] …`), the local scroll logs `[wheel] scroll → …`, the
// scrollbar thumb `top` is the scroll DOM-state, and `document.activeElement` is the focus
// DOM-state. These lock the live-MCP proof as regression gates (the DECISIONS are also
// unit-tested; this is the real DOM glue the node suite can't run). A wheel is dispatched
// as a LINE-mode WheelEvent (one physical notch) for determinism.
test.describe("S16 input + wheel + focus wiring (#133)", () => {
  const wheelNotch = (page: import("@playwright/test").Page, deltaY: number) =>
    page.evaluate((dy) => {
      const c = document.querySelector("#term") as HTMLElement;
      const r = c.getBoundingClientRect();
      c.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: dy,
          deltaMode: 1, // LINE
          bubbles: true,
          cancelable: true,
          clientX: r.left + 50,
          clientY: r.top + 50,
        }),
      );
    }, deltaY);
  // The scrollbar thumb's `top` (%) is the scroll DOM-state; the track is a body-level
  // absolute div with a right edge and a thumb child.
  const thumbTop = (page: import("@playwright/test").Page) =>
    page.evaluate(() => {
      const track = [...document.querySelectorAll("div")].find(
        (d) =>
          d.style.position === "absolute" &&
          d.style.right === "0px" &&
          d.style.height === "100%" &&
          d.querySelector("div"),
      );
      const t = track?.querySelector("div") as HTMLElement | undefined;
      return t ? parseFloat(t.style.top) : null;
    });

  test("clicking the terminal focuses its hidden IME textarea (#116)", async ({ page }) => {
    // The real input target is a hidden textarea (a canvas can't receive composition
    // events); a pointer-down on the canvas focuses it via the container.
    expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe("TEXTAREA");
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    expect(await page.evaluate(() => document.activeElement?.tagName)).toBe("TEXTAREA");
  });

  test("keystrokes and paste reach the input sink", async ({ page }) => {
    const intents: string[] = [];
    page.on("console", (m) => {
      if (m.text().includes("[input]")) intents.push(m.text());
    });
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("a");
    expect(intents.some((l) => l.includes('[input] key {"type":"char","char":"a"}'))).toBe(true);
  });

  test("wheel scrolls scrollback (normal buffer): thumb moves up, offset climbs", async ({
    page,
  }) => {
    const scrolls: number[] = [];
    page.on("console", (m) => {
      const n = m.text().match(/\[wheel\] scroll → displayOffset (\d+)/);
      if (n) scrolls.push(Number(n[1]));
    });
    // Scrollback needs the log to exceed the viewport (a line appends every 300ms).
    // Retry a wheel-up until the demo actually scrolls into history (offset > 0) — this
    // is scrollback-size-independent, unlike an absolute thumb threshold.
    await expect
      .poll(
        async () => {
          await wheelNotch(page, -4);
          return scrolls.at(-1) ?? 0;
        },
        { timeout: 25_000, intervals: [400] },
      )
      .toBeGreaterThan(0);
    // DOM state: two quick up-notches (back-to-back reads minimise append drift) lower
    // the thumb `top` toward the track top (older content), OR it's already pinned there.
    const before = (await thumbTop(page))!;
    await wheelNotch(page, -6);
    await wheelNotch(page, -6);
    const after = (await thumbTop(page))!;
    expect(after).toBeLessThanOrEqual(before); // thumb rose (or pinned at the top)
    expect(scrolls.at(-1)!).toBeGreaterThan(0); // still scrolled into history
  });

  test("App mouse ON routes the wheel to the app, not scrollback", async ({ page }) => {
    const intents: string[] = [];
    const scrolls: string[] = [];
    page.on("console", (m) => {
      if (m.text().includes("[input] mouse")) intents.push(m.text());
      if (m.text().includes("[wheel] scroll")) scrolls.push(m.text());
    });
    await expect.poll(() => thumbTop(page), { timeout: 15_000 }).toBeLessThan(90);
    await page.getByRole("button", { name: "App mouse: OFF" }).click();
    const before = (await thumbTop(page))!;
    await wheelNotch(page, -3);
    expect(intents.some((l) => l.includes("wheelUp"))).toBe(true); // reported to the app
    expect(scrolls).toHaveLength(0); // did NOT scroll scrollback
    expect(await thumbTop(page)).toBe(before); // thumb unmoved
  });

  test("alt-screen wheel (no scrollback) becomes cursor keys, not a scroll", async ({ page }) => {
    const intents: string[] = [];
    const scrolls: string[] = [];
    page.on("console", (m) => {
      if (m.text().includes("[input] key")) intents.push(m.text());
      if (m.text().includes("[wheel] scroll")) scrolls.push(m.text());
    });
    await page.getByRole("button", { name: "Alt screen: OFF" }).click();
    await wheelNotch(page, -3); // up
    await wheelNotch(page, 3); // down
    expect(intents.some((l) => l.includes('{"type":"up"}'))).toBe(true);
    expect(intents.some((l) => l.includes('{"type":"down"}'))).toBe(true);
    expect(scrolls).toHaveLength(0); // no scrollback scroll on the alt screen
  });

  test("a shift-wheel produces no report and lets native scroll through", async ({ page }) => {
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    // Capture only wheel-derived intents/scrolls (a focus intent from the click above is
    // expected and unrelated); a shift-wheel must yield none of these.
    const signals: string[] = [];
    page.on("console", (m) => {
      const t = m.text();
      if (t.includes("[input] mouse") || t.includes("[input] key") || t.includes("[wheel]")) {
        signals.push(t);
      }
    });
    const prevented = await page.evaluate(() => {
      const c = document.querySelector("#term") as HTMLElement;
      const r = c.getBoundingClientRect();
      const ev = new WheelEvent("wheel", {
        deltaY: -4,
        deltaMode: 1,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
        clientX: r.left + 50,
        clientY: r.top + 50,
      });
      return !c.dispatchEvent(ev); // true iff preventDefault was called
    });
    expect(prevented).toBe(false); // native scroll not suppressed (WheelScroller bailed)
    expect(signals).toHaveLength(0); // no spurious app report / scroll
  });
});

// #116 (S7): IME composition through the hidden textarea. Headless can't run a real IME,
// but the demo dispatches the same composition/keydown events a Korean IME fires — the
// real CompositionController + Terminal wiring run, and the committed `text` intent is the
// DOM-observable proof (the demo logs `[input] text "…"`). The committed value comes from
// the textarea, never the (misleading) event data — the whole point of the mechanism.
test.describe("S7 IME composition (#116)", () => {
  // Focus the textarea (via a canvas click) and drive a composition that commits `committed`
  // while the last update `data` lies — returns the `[input] text` payloads that were logged.
  const compose = (page: import("@playwright/test").Page, data: string, committed: string) =>
    page.evaluate(
      ({ data, committed }) => {
        const ta = document.querySelector("textarea")!;
        (document.querySelector("#term") as HTMLElement).dispatchEvent(
          new MouseEvent("mousedown", { bubbles: true }),
        ); // focus the textarea
        ta.dispatchEvent(new CompositionEvent("compositionstart"));
        ta.dispatchEvent(new CompositionEvent("compositionupdate", { data }));
        ta.value = committed;
        ta.selectionStart = committed.length;
        ta.selectionEnd = committed.length;
        ta.dispatchEvent(new CompositionEvent("compositionend", { data }));
      },
      { data, committed },
    );

  test("commits the textarea value as a text intent, ignoring the event data", async ({
    page,
  }) => {
    const texts: string[] = [];
    page.on("console", (m) => {
      const x = m.text().match(/\[input\] text "(.+)"/);
      if (x) texts.push(x[1]);
    });
    // The last update data ("니") lies (jongseong migrated); the textarea holds "아니".
    await compose(page, "니", "아니");
    await expect.poll(() => texts).toContain("아니");
    expect(texts).not.toContain("니"); // never the event data
  });

  test("Enter finalizes an in-progress composition before sending the key", async ({ page }) => {
    const intents: string[] = [];
    page.on("console", (m) => {
      const t = m.text();
      if (t.includes("[input] text") || t.includes("[input] key")) intents.push(t);
    });
    await page.evaluate(() => {
      const ta = document.querySelector("textarea")!;
      (document.querySelector("#term") as HTMLElement).dispatchEvent(
        new MouseEvent("mousedown", { bubbles: true }),
      );
      ta.dispatchEvent(new CompositionEvent("compositionstart"));
      ta.dispatchEvent(new CompositionEvent("compositionupdate", { data: "가" }));
      ta.value = "가";
      ta.selectionStart = 1;
      ta.selectionEnd = 1;
    });
    await page.waitForTimeout(20); // let the compositionupdate end-tracking settle
    await page.evaluate(() => {
      document
        .querySelector("textarea")!
        .dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    });
    await expect.poll(() => intents.filter((t) => t.includes('text "가"'))).toHaveLength(1);
    // The commit precedes the Enter key in the intent stream (composition sent first).
    const commitIdx = intents.findIndex((t) => t.includes('text "가"'));
    const enterIdx = intents.findIndex((t) => t.includes('"type":"enter"'));
    expect(commitIdx).toBeGreaterThanOrEqual(0);
    expect(enterIdx).toBeGreaterThan(commitIdx);
  });

  test("the hidden textarea is cleared after a commit (no unbounded growth)", async ({ page }) => {
    await compose(page, "한", "한");
    await expect.poll(() => page.evaluate(() => document.querySelector("textarea")?.value)).toBe("");
  });

  test("focus returns to the input textarea after the accessible view closes", async ({ page }) => {
    // The input target moved to the hidden textarea; focus-restore paths must target it,
    // not the (now inert) canvas — else typing/IME is dead after the overlay closes.
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.getByRole("button", { name: /Accessible view/ }).click();
    await expect(page.locator("[role='document']")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[role='document']")).toBeHidden();
    expect(await page.evaluate(() => document.activeElement?.tagName)).toBe("TEXTAREA");
  });
});

// #117 (S13): consumer event surface. The demo pushes title/bell/cwd through the source's
// event channel (a real backend drains them from core); the widget routes each to the
// consumer handlers. onTitle drives the real document title (DOM-observable); onBell/onCwd
// are proven via their console signal (fire-and-forget, no DOM effect of their own).
test.describe("S13 consumer events (#117)", () => {
  test("Set title drives the document title (onTitle → xterm onTitleChange parity)", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Set title" }).click();
    await expect(page).toHaveTitle("justerm — tab 1");
    await page.getByRole("button", { name: "Set title" }).click();
    await expect(page).toHaveTitle("justerm — tab 2"); // a second event re-fires the handler
  });

  test("Bell and Set cwd fire their handlers", async ({ page }) => {
    const events: string[] = [];
    page.on("console", (m) => {
      const t = m.text();
      if (t.includes("[event]")) events.push(t);
    });
    await page.getByRole("button", { name: "Bell" }).click();
    await page.getByRole("button", { name: "Set cwd" }).click();
    await expect.poll(() => events.some((e) => e === "[event] bell")).toBe(true);
    await expect
      .poll(() => events.some((e) => e.startsWith("[event] cwd") && e.includes("file://")))
      .toBe(true);
  });
});

// #114: on container resize the demo auto-fits — computes cols/rows from the CSS box +
// cell size and drives a debounced resize intent (the demo logs `[fit] resize CxR`). Proven
// live: the ResizeObserver + FitController + proposeDimensions path runs in real Chromium,
// which the DOM-less unit tests can't exercise. Shrinking the viewport yields fewer cols.
test("container resize drives a debounced fit intent with a smaller grid (#114)", async ({
  page,
}) => {
  const fits: string[] = [];
  page.on("console", (m) => {
    const t = m.text();
    if (t.includes("[fit] resize")) fits.push(t);
  });
  const colsOf = (line: string): number => Number(line.match(/resize (\d+)x/)?.[1]);

  // The observer fires once on mount with the initial (large) viewport.
  await expect.poll(() => fits.length).toBeGreaterThan(0);
  const firstCols = colsOf(fits[0]);

  // Shrink the viewport → smaller box → a new, smaller grid (debounced ~100ms).
  await page.setViewportSize({ width: 360, height: 300 });
  await expect.poll(() => fits.length).toBeGreaterThan(1);

  const lastCols = colsOf(fits[fits.length - 1]);
  expect(lastCols).toBeGreaterThanOrEqual(2); // MINIMUM_COLS
  expect(lastCols).toBeLessThan(firstCols); // the fit tracked the smaller box
});
