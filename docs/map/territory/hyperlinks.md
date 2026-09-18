# Territory — hyperlinks

## What it is

Two unrelated ways a cell becomes clickable, deliberately kept apart:

- **OSC 8** — the application *declares* a link, so the engine stores which cells carry which URI
- **Plain-text URL detection** — nobody declared anything, so the consumer runs a regex over
  assembled text and decides

The first is per-cell state that survives in the buffer; the second is a policy applied to a snapshot.
They share a name and almost nothing else.

## Governing decisions

**No ADR.** Three maintainer calls, made on #934 — judgements, not derivations, so a stronger
argument does not reopen them; the maintainer does. They were shown each option's consequences after
an adversarial pass and a refuting pass over the options, both checked against source:

- **Plain-text URLs reach a frame-mode widget through a consumer-wired `LinkPort`** — the backend
  answers the hovered row's logical line from core on demand. Rejected: joining rows web-side (loses
  a URL that wraps across the viewport's top edge, and supersedes ADR-0017's rejection of it),
  shipping edge rows on the wire (a VERSION bump, paid every frame), and core running the pattern.
- **The widget's cell mirror stores each cell's URI**, so OSC 8 links stay whole across Partial
  frames with no wire change. The accepted cost: two opens of one URI that *touch* merge in the widget.
  (What was shown at the time said "only adjacent opens merge" — the first implementation merged every
  cell of a URI across the viewport, which the e2e caught; the contiguity rule below is what makes the
  consequence that was shown true.)
- **Hover shows the pointer cursor and an underline**, the underline through a new renderer setter.

Two more, made after the implementation's check pass, on derivations it had produced:

- **Only pointer motion asks the port** — kept over "also ask on frames", shown with the measured
  100 asks per 100 scrolling frames of the first cut and the cost of the alternative (a new link
  under a resting pointer appears only on its next motion).
- **An already-underlined cell shows hover by the cursor alone** — kept over ghostty's switch to a
  double underline.

Declined on #934: a core one-row logical-line entry point — no consumer calls `lineAt` yet; see
*Known holes*.

What those calls did not cover is recorded under *Design model* as derivations.

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  the split above *is* this record applied twice: storing declared links needs the buffer, so it is
  core; deciding what counts as a URL is policy, so it is not. (Its "no regex dependency" ground was
  withdrawn by its own 2026-07-22 amendment; the policy ground stands.)
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — why OSC 8 is **not** a consumer event: a hyperlink is per-cell state, not a point-in-time
  notification. The clearest worked example of its state-versus-event rule

## Design model

- **A declared link is a shared string, and it lives on the row.** The cell carries `LINK_PRESENT`
  and the row's link map holds a column-keyed `Arc<str>` — so one OSC 8 open covering a thousand
  cells is a thousand map entries and one allocation.
- **The consumer gets an owned handle, not a borrow.** `Engine::link_at` returns `Hyperlink` — a
  thin `Arc` wrapper — because a borrow into the row's map cannot outlive `&Engine`, and the caller
  that needs one (a hover handler, while output keeps arriving) would copy the string instead: 62.6 ns
  against the handle's 17.9 ns.
- **One dedup rule, stated in two directions.** Never merge on URI alone; always merge on an `id=`
  the application declared. Both halves come from the same reference sentence pair, and shipping only
  the first is what #635 closed. The group key is `id` **and** URI (a reused id aimed at a new target
  is not the same link), an empty `id=` value is no id at all, and `params` is a `:`-separated list in
  which `id` may sit anywhere — each of the three is a place a reasonable reading goes wrong, so each
  is pinned by its own test rather than by a comment.
- **The group registry is `Weak`, and that is the whole reason grouping did not undo #628.** An
  id→link map holding strong references would make every id'd link immortal for the life of the
  `Term` — the exact defect #628 removed, re-entering through the door grouping opens. A dangling key
  is the *correct* answer, not a hole: the link it named has left the buffer, so a later open of that
  id is a new link. The sweep for dangling keys is affordable here for the reason #628's rejected
  sweep option was not — staleness is O(1) (`Weak::strong_count`) instead of an O(buffer) walk.
- **Two opens of one URI are two links, and nothing public can currently tell.** `uri() == uri()`
  says "same" where the engine says "different", and the `Arc::ptr_eq` accessor that would answer
  properly is deliberately unshipped — no consumer asks yet, and adding a method later is not a
  breaking change where changing `Hyperlink`'s shape would be. Pinned in-crate by
  `two_opens_of_one_uri_are_two_links`.
- **The row is the unit of lifetime, which is why there is almost no reclamation code.** The URI dies
  with the last row holding it (row reuse, scrollback eviction, reflow dropping a row) — plus one
  explicit purge where a cell is blanked *in place* (`Row::purge_side_maps`), because there no row
  event fires and the map would go on owning the string behind a cleared bit. It was an index
  into a buffer-wide `hyperlink_pool` until #628, and that pool was **never** reclaimed — the same
  defect the combining map had and lost when #45 deleted `grapheme_pool`. Links kept it only because
  #46 mirrored xterm's `_dataByLinkId` registry and ported the id-minting half without the delete
  half.
- **`Arc`, not inlining, is the one way links differ from combining marks.** A cluster is per-cell
  and unique, so the sibling map stores it by value; a URI is shared across an open's cells, so
  storing it by value would duplicate it per cell — the shape #621 measured at +171…403% on the wire
  and rejected there for the same reason. `Rc` is not an option: `Engine` is `Send + Sync`.
- **`LINK_PRESENT` occupies xterm's `HAS_EXTENDED` slot**, which is the sibling of the combining-mark
  bit: both are presence flags gating a row-keyed side map.
- **The wire indexes per frame, and that half is unchanged.** A frame carries only the URIs it needs,
  numbered into frame-local `link_table` positions — keyed by the `Arc`'s identity since #628, so one
  open is one entry however many cells it covers. A consumer never sees an engine-side handle, and a
  decoded `Span`'s `links` is a *frame-local* index that must never be fed back to the engine.
- **Detection is the inverse arrangement.** The engine assembles the viewport's logical-line text plus
  a per-character cell map — it has the whole buffer and the consumer does not — and the consumer runs
  the regex and `new URL()` validation over that text, mapping matches back through the cells.
- **Neither path knows about the other.** A detected URL is not stored, and a declared link is not
  re-validated. Where one cell is both, the widget follows the OSC 8 link (`LinkController` puts
  declared links first); no engine-side artifact says so.

### In the widget (#934) — derivations

- **The widget owns hover and click; the consumer owns opening.** A link is live exactly where a
  press would stay local (`pressGoesToApp`, Shift overriding), and hover asks that same question —
  plus the consumer's `activates` gate, so a Ctrl-to-follow consumer gets no underline on a plain
  hover. The deliberate divergence from xterm.js: its Linkifier activates under mouse tracking too;
  #934 required that a press the application takes never opens a link.
- **A click is a press and release on the same cell of one link.** Leaving the pressed cell during
  the press makes the gesture a selection, so dragging to copy a URL does not open it. A double
  click's second press presses no link; its first still opens, as in every reference.
- **A web-side OSC 8 link is a contiguous run of one URI**, continuing past a row's end only where
  the row soft-wraps. The frame-local index cannot identify an open across frames, so contiguity is
  the proxy; it is what keeps eight separate "select" links on eight rows from hovering as one.
- **Only pointer motion asks the port.** A frame drops or carries cached answers but never asks,
  so the questions follow the rows the pointer crosses, whatever the output rate — a pointer
  resting over streaming output asks nothing, and shows a link under it again on its next motion.
  This is how "no per-frame IPC call" (#934) is met: the first cut asked once per frame under a
  resting pointer (measured 100 asks in 100 scrolling frames).
- **An answer is kept only while the mirror shows it cell for cell**: every character in the cell it
  names, every other cell of its rows blank, the rows it spans wrapping into each other and the rows
  around it not. Text-per-row was not enough — a row that starts to wrap keeps its text, and the
  cached cut-off URL would open (the check pass found it in two independent reads).
- **A whole-screen scroll carries the answers with the rows**; a region scroll leaves the check to
  drop what moved.
- **A pointer cell outside the mirror is no cell**, read once at `LinkTracker.lead` so hover, press,
  drag and release all agree. The pointer's grid is the consumer's `getGeometry`, the mirror's the
  last frame's; they differ after a resize until the next frame, and permanently in a harness with
  no backend to resize (PenTerm's check, 80×21 against 40×8 frames — the #934 reopen). A column
  past the width does not throw: row-major indexing reads the next row's cell, so both axes are
  bounded.
- **The hovered underline is a single underline drawn where the cell has none**, through the flags
  the line reads (`line_flags`), so it follows every colour rule an `SGR 4` underline does. A cell
  already underlined keeps its own style, so hover shows only as the pointer cursor there.

## Code

- `justerm-core/src/cell.rs` — `LINK_PRESENT`
- `justerm-core/src/grid.rs` — `Links`, the row's column-keyed link map
- `justerm-core/src/term.rs` — `Term::current_link` and the per-frame remap into `link_table`
- `justerm-core/src/serialize.rs` — `Frame`'s `link_table`
- `justerm-web/src/links.ts` — the URL regex and validation policy, over
  [logical lines](logical-lines.md); `LinkController`, `LinkPort`, `LinkOptions`
- `justerm-web/src/link-tracker.ts` — `LinkTracker`, the widget's link state, and `hoverSpans`
- `justerm-web/src/cell-mirror.ts` — `CellMirror.osc8Links`, the per-cell URI column
- `justerm-web/src/pointer.ts` — `PointerRouter`'s link half and `cellAt`
- `justerm-web/src/terminal.ts` — `TerminalOptions.links`, the hover presentation
- `justerm-renderer/src/frame.rs` — `line_flags` in `pack_instances`; `overlay.rs` —
  `Overlay::is_link_hovered`; `webgl.rs` — `set_link_hover`

## Reference behaviour

[OSC 8 hyperlinks — where the URI lives, and what frees it](../../agents/reference-facts.md#osc-8-hyperlinks--where-the-uri-lives-and-what-frees-it-628635-verified-2026-07-30)
— added by #628, which is when the area first needed them. Two questions with **different** answers:
all three references free the storage (3:0, and justerm was the outlier until #628), while only
xterm.js groups by `id=` (1:2 — so #635 was a conformance item against the one reference that has the
feature, not a divergence from a consensus, and it is closed). The section's second half records how
that reference *parses* `id=`, row by row, because three separate readings of it are wrong in ways a
single-parameter test cannot see.

Read the section's own warning before citing it: taking only the first row of each reference gives
*"they all keep a registry"*, which is how #46 arrived at a permanent pool — the reclamation half
does not appear at the site where the id is minted.

Still unpinned: `LINK_PRESENT` is described as occupying xterm's `HAS_EXTENDED` slot — a concrete
claim about another implementation's bit layout, in a comment, with no row.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the link map is one of the three, under
  the same presence-bit discipline: read only through the gate, ride with the row, and a write that
  clears the cell owes the bit rather than the map
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — `link` and
  `linkTable` meet a consumer as accessors, so `osc8Links` destructures them once before walking
  the span directory. That is load-bearing, not style
- [a pointer coordinate is bounded by its producer](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — the widget's link converter owes the bound, and refuses where its siblings clamp

## Blast radius

- [logical lines](logical-lines.md) — detection consumes that shape, including its off-screen context
  rows, so a change to trimming changes which URLs match
- [wire format](wire-format.md) — `link_table` is a per-frame side table, and the remap is what keeps
  the cell record fixed-width
- [events & replies](events-and-replies.md) — the boundary partner: OSC 8 is the worked example of
  what is *not* an event
- [reflow](reflow.md) · [soft wrap](soft-wrap.md) — the link map rides the row through both, and a URL
  detected across a wrap join spans rows the consumer must highlight separately

## Known holes / open

- **Zero governing records** for either path, including the row-owned `Arc` design that makes a
  repeated URI cheap and bounds its lifetime.
- **Whether a *public* accessor should answer "same link?"** is still open, and #635 made the question
  live rather than settling it. Grouping now works, so two runs genuinely are one link — but the only
  way to observe that from outside the crate is a decoded frame's `link_table` index, which is
  frame-local and belongs to the wire. `Hyperlink` still exposes `uri()` alone, and the `Arc::ptr_eq`
  accessor that would answer directly stays deliberately unshipped (no consumer asks). A consumer that
  wanted to ask *in engine coordinates* currently cannot.
- **The widget's overlap rule has no engine-side counterpart.** OSC 8 wins over a detected URL on one
  cell in `LinkController`, and only there.
- **A port answer's off-screen context is never checked.** The mirror holds the viewport only, so a
  line whose head sits above the top (or whose tail sits below the bottom) is kept on what the
  viewport shows of it. Reaching a wrong link needs identical on-screen text over a changed off-screen
  part.
- **One `lineAt` costs the backend a whole `viewport_logical_lines`**, since core has no one-row
  entry point; the question count is bounded by motion, the cost per question is not.
- **A widget with a11y wired keeps two viewport mirrors**, one each, applying every frame.
- **The `HAS_EXTENDED` claim is unpinned**, and it is a statement about another project's bit layout.
- **Detection is viewport-only by construction.** A URL entirely in scrollback is never detected,
  because the consumer only ever sees assembled *viewport* lines — a limitation no document states.
