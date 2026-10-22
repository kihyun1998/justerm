# Territory — hyperlinks

## What it is

Two unrelated ways a cell becomes clickable, deliberately kept apart:

- **OSC 8** — the application *declares* a link, so the engine stores which cells carry which URI
  and ships it on the frame.
- **Plain-text URL detection** — nobody declared anything, so the consumer runs a regex over
  assembled [logical lines](logical-lines.md) and decides.

The first is per-cell state that survives in the buffer; the second is a policy applied to a
snapshot. They share a name and almost nothing else. The widget's hover, click and underline over
both live here too.

## Governing decisions

**None.** No ADR. Three maintainer calls on #934 (judgements, not derivations — a stronger argument
does not reopen them; the maintainer does), shown each option's consequences after an adversarial and
a refuting pass:

- **Plain-text URLs reach a frame-mode widget through a consumer-wired `LinkPort`** — the backend
  answers the hovered row's logical line from core on demand. Rejected: joining rows web-side (loses a
  URL that wraps across the viewport's top edge, and supersedes ADR-0017's rejection of it), shipping
  edge rows on the wire (a version bump paid every frame), and core running the pattern.
- **The widget's cell mirror stores each cell's URI**, so OSC 8 links stay whole across Partial
  frames with no wire change. Accepted cost: two opens of one URI that *touch* merge in the widget.
  (What was shown said "only adjacent opens merge"; the first implementation merged every cell of a
  URI across the viewport, which the e2e caught — the contiguity rule below makes the shown
  consequence true.)
- **Hover shows the pointer cursor and an underline**, the underline through a new renderer setter.

Two more after the implementation's check pass: **only pointer motion asks the port** (kept over
"also ask on frames"; the first cut measured 100 asks per 100 scrolling frames, and the alternative's
cost is that a new link under a resting pointer appears only on its next motion), and **an
already-underlined cell shows hover by the cursor alone** (kept over ghostty's double underline).
Declined on #934: a core one-row logical-line entry point (see Known holes). The 10 MB URI cap was
kept by the maintainer on 2026-10-08 (#1038).

Adjacent, not governing:

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  the split above *is* this record applied twice: storing declared links needs the buffer, so it is
  core; deciding what counts as a URL is policy, so it is not. (Its "no regex dependency" ground was
  withdrawn by its own 2026-07-22 amendment; the policy ground stands.)
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — why OSC 8 is **not** a consumer event: a hyperlink is per-cell state, not an occurrence.

## Design model

Status: no record; the engine half below comes from the source (read at 7bb31f7) and #628/#635/#1038,
the widget half from #934's derivations.

**Engine — declared links**

- **A declared link is a shared string, and it lives on the row.** The cell carries the presence bit
  (`BG_LINK`, called LINK_PRESENT in the docs) and the row's `Links` map holds a column-keyed
  `LinkUri` — one OSC 8 open covering a thousand cells is a thousand map entries and one allocation.
  While an open is active the `Term` holds it as `current_link`.
- **The consumer gets an owned handle, not a borrow.** `Engine::link_at` returns `Hyperlink`, a thin
  `Arc` wrapper, because a borrow into the row's map cannot outlive `&Engine` and a hover handler
  holding one across output would copy the string instead (62.6 ns against the handle's 17.9 ns,
  #628).
- **`uri()` is the URI exactly as declared — with one exception**: a URI containing 14 or more
  unencoded `;` arrives cut short, because `vte` passes at most 16 OSC fields; the cut is not marked
  (`Hyperlink::uri` doc). Fewer are rejoined (`a_uri_keeps_its_unencoded_semicolons`).
- **One dedup rule, stated in two directions.** Never merge on URI alone; always merge on an `id=` the
  application declared (#635 shipped the second half). The group key is `id` **and** URI, an empty
  `id=` is no id, and `id` may sit anywhere in the `:`-separated params — each pinned by its own test
  (`term.rs` — `two_opens_of_one_uri_are_two_links`, `the_same_id_and_uri_group_into_one_link`,
  `the_same_id_with_a_different_uri_stays_two_links`, `an_empty_id_value_is_no_id_at_all`,
  `the_id_param_is_found_among_other_params`).
- **The group registry is `Weak`, which is why grouping did not undo #628.** An id→link map of strong
  references would make every id'd link immortal for the life of the `Term`. A dangling key is the
  correct answer: the link left the buffer, so a later open of that id is a new link. The sweep is
  affordable because staleness is `O(1)` (`Weak::strong_count`), amortised by doubling the threshold
  (`LINK_IDS_FIRST_SWEEP`) — pinned by `the_id_registry_does_not_keep_a_link_alive`.
- **A dangling key must not hold the URI, and until #1038 it held two copies.** Measured on 727865d:
  103 MiB retained after `ED 3` + `ED 2` erased every cell of 50 1-MiB id'd links. The key was
  `"id;;uri"`; it is now `(id, hash of the URI)`, confirmed against the live link's URI so a collision
  reads as a miss. The value was a `Weak<str>`, whose allocation *is* the URI bytes;
  `LinkUri = Arc<Box<str>>` puts the bytes behind the box, freed with the last strong reference while
  the weak keeps only the `ArcInner`. Cost: one pointer hop per link read and one extra allocation per
  *open*. After: 3 MiB.
- **Outside input is bounded at the open (#1038).** A URI past `MAX_LINK_URI` (10 000 000 bytes)
  makes the whole sequence a no-op — xterm.js's `PAYLOAD_LIMIT` and its failure shape, never a
  truncated destination; an `id=` past `MAX_LINK_ID` (250, VTE's value) is no id. VTE/iTerm2's 2083
  was tried first and dropped: it cuts against #621, whose `wire_capacity.rs` pins a URI past
  `u16::MAX` as legitimate, and URI memory grows 1:1 with input (`REP` and every covered cell share one
  allocation), so the key copy was the only multiplier. The count of live links is bounded only by the
  buffer's cells (Known holes).
- **Two opens of one URI are two links, and nothing public can tell.** `uri() == uri()` says "same"
  where the engine says "different"; an `Arc::ptr_eq` accessor is deliberately unshipped (adding one
  later is not breaking; changing `Hyperlink`'s shape would be).
- **The row is the unit of lifetime, so there is almost no reclamation code.** The URI dies with the
  last row holding it (row reuse, eviction, reflow dropping a row), plus one explicit purge where a
  cell is blanked *in place* (`Row::purge_side_maps`), where no row event fires
  (`an_erased_cell_releases_its_uri_not_only_its_bit`, `a_link_evicted_from_the_buffer_stops_being_stored`).
  It was an index into a never-reclaimed buffer-wide `hyperlink_pool` until #628 — #46 had ported
  xterm's `_dataByLinkId` id-minting half without the delete half.
- **`Arc`, not inlining, is the one way links differ from combining marks.** A cluster is per-cell and
  unique; a URI is shared across an open's cells, so storing it by value would duplicate it per cell
  (#621 measured that shape on the wire at +171…403 %). `Rc` is out: `Engine` is `Send + Sync`
  (`the_engine_stays_send_and_sync`).
- **The presence bit occupies xterm's `HAS_EXTENDED` slot** (bit 28 of the bg word, per the `cell.rs`
  comment), the sibling of the combining-mark bit: both gate a row-keyed side map.
- **The wire indexes per frame.** `Term::frame` numbers each distinct open once into the frame-local
  `link_table`, keyed by the `Arc`'s identity (#26, #628) and sized by the frame — it used to zero a
  `hyperlink_pool.len() + 1` vector per frame (10 µs with 100 000 opens retained). A decoded `Span`'s
  `links` is a *frame-local* index that must never be fed back to the engine.

**Detection — the inverse arrangement**

- The engine assembles the viewport's logical-line text plus a per-character cell map
  (`viewport_logical_lines`) — it has the whole buffer and the consumer does not — and the consumer
  runs the regex and `new URL()` validation over it, mapping matches back through the cells. PenTerm
  wires the port by picking, from that answer, the line covering the asked row (penterm 7dadc3c95
  `src-tauri/src/engine/links.rs` — `line_at`, #2101).
- **Neither path knows about the other.** A detected URL is not stored, and a declared link is not
  re-validated. Where one cell is both, the widget follows the OSC 8 link (`LinkController` puts
  declared links first); no engine-side artifact says so.

**Widget (#934) — derivations**

- **The widget owns hover and click; the consumer owns opening.** A link is live exactly where a
  press would stay local (`pressGoesToApp`, Shift overriding), plus the consumer's `activates` gate,
  so a Ctrl-to-follow consumer gets no underline on a plain hover. Deliberately unlike xterm.js, whose
  Linkifier activates under mouse tracking too: a press the application takes never opens a link.
- **A click is press and release on the same cell of one link.** Leaving the pressed cell makes the
  gesture a selection, so dragging to copy a URL does not open it. A double click's first press still
  opens, as in every reference.
- **A web-side OSC 8 link is a contiguous run of one URI** (`CellMirror.osc8Links`), continuing past a
  row's end only where the row soft-wraps. The frame-local index cannot identify an open across frames,
  so contiguity is the proxy.
- **Only pointer motion asks the port**, so questions follow the rows the pointer crosses whatever the
  output rate — the "no per-frame IPC call" requirement of #934.
- **An answer is kept only while the mirror shows it cell for cell**: every character in its cell,
  every other cell of its rows blank, its rows wrapping into each other and the rows around it not.
  Text-per-row was not enough — a row that starts to wrap keeps its text, and the cached cut-off URL
  would open (found twice by the check pass). A whole-screen scroll carries answers with the rows; a
  region scroll leaves the check to drop what moved.
- **A pointer cell outside the mirror is no cell**, read once at `LinkTracker`'s `lead` so hover,
  press, drag and release agree. The pointer's grid is the consumer's `getGeometry`, the mirror's the
  last frame's; they differ after a resize until the next frame, and permanently in a harness with no
  backend to resize (PenTerm's check, 80×21 against 40×8 frames — the #934 reopen). Row-major indexing
  would read the next row's cell, so both axes are bounded.
- **The hovered underline is a single underline drawn where the cell has none**, through the flags the
  renderer's line pass reads (`pack_instances`), so it follows every colour rule an `SGR 4` underline
  does; an already-underlined cell shows hover only by the cursor.

## Code

- `justerm-core/src/cell.rs` — `BG_LINK`
- `justerm-core/src/grid.rs` — `LinkUri`, `Links`, `Row::link_at`, `Row::purge_side_maps`
- `justerm-core/src/term.rs` — `Hyperlink`, `MAX_LINK_URI`, `MAX_LINK_ID`, `LINK_IDS_FIRST_SWEEP`, `Term`'s `current_link` / `link_ids`, `Term::frame` (the `link_remap` into `link_table`)
- `justerm-core/src/term/dispatch.rs` — `Term::link_for_id`, the OSC 8 arm
- `justerm-core/src/lib.rs` — `Engine::link_at`, `Engine::viewport_logical_lines`
- `justerm-core/src/serialize.rs` — `Frame`'s `link_table`, `Span`'s `links`
- `justerm-core/tests/hyperlink.rs` · `wire_capacity.rs`
- `justerm-web/src/links.ts` — `osc8Links`, `LinkController`, `LinkPort`, `LinkOptions`; the URL regex and validation policy
- `justerm-web/src/link-tracker.ts` — `LinkTracker`, `hoverSpans`
- `justerm-web/src/cell-mirror.ts` — `CellMirror`'s `osc8Links`, the per-cell URI column
- `justerm-web/src/pointer.ts` — `pressGoesToApp`, `cellAt`, `PointerRouterDeps`
- `justerm-renderer/src/frame.rs` — `pack_instances`; `overlay.rs` — `Overlay::is_link_hovered`; `webgl/grid_state.rs` — `set_link_hover`

## Reference behaviour

- [OSC 8 hyperlinks — where the URI lives, and what frees it](../../agents/reference-facts.md#osc-8-hyperlinks--where-the-uri-lives-and-what-frees-it-628635-verified-2026-07-30)
  — two questions with different answers: all three references free the storage (3:0; justerm was
  the outlier until #628), while only xterm.js groups by `id=` (1:2, so #635 was a conformance item
  against the one reference with the feature). Read its warning before citing it: the first row of
  each reference reads "they all keep a registry", which is how #46 arrived at a permanent pool.
- [How xterm.js parses `id=`](../../agents/reference-facts.md#how-xtermjs-parses-id-and-the-three-places-a-reasonable-reading-goes-wrong-635-verified-2026-07-30)
  — the three readings that go wrong.
- [Per-cell payload length](../../agents/reference-facts.md#per-cell-payload-length--three-of-four-leave-a-cluster-unbounded-the-binding-one-caps-it-and-a-uri-is-a-different-answer-621-verified-2026-07-29-xterm-row-added-867-2026-09-08)
  — the URI axis behind `MAX_LINK_URI`.

Still unpinned: the `HAS_EXTENDED` claim is a statement about another implementation's bit layout,
held in a `cell.rs` comment with no row.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the link map is one of the three,
  under the same presence-bit discipline: read only through the gate, ride with the row, and a write
  that clears the cell owes the bit rather than the map.
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — `link` and
  `linkTable` meet a consumer as accessors, so `osc8Links` destructures them once before walking the
  span directory. Load-bearing, not style.
- [a pointer coordinate is bounded by its producer](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — the widget's link converter owes the bound, and refuses where its siblings clamp.

## Blast radius

- [logical lines](logical-lines.md) — detection consumes that shape including its off-screen context
  rows, so a change to trimming or joining changes which URLs match.
- [wire format](wire-format.md) — `link_table` is interned and per frame; the remap is what keeps the
  cell record fixed-width, and the presence bit must be re-armed on decode.
- [events & replies](events-and-replies.md) — the boundary partner: OSC 8 is the worked example of
  what is *not* an event.
- [reflow](reflow.md) · [soft wrap](soft-wrap.md) — the link map rides the row through both, and a
  detected URL across a wrap join spans rows the consumer must highlight separately.
- [input encoding](input-encoding.md) — "live" means "a press stays local", the same mouse-mode mask
  pointer routing reads; a change to it changes where a link can be clicked.
- [cell compositing](cell-compositing.md) — the hover underline reuses the `SGR 4` line flags; a
  change to line colour rules changes the hover.

## Known holes / open

- **Live URIs grow 1:1 with input, kept on purpose (#1038).** Distinct one-cell links hold up to
  `MAX_LINK_URI` bytes each for as long as their rows live, and a visible 9 MB link is re-sent in
  every full-damage frame. The maintainer kept the cap on 2026-10-08, shown three options (keep it;
  VTE's 2083, rewriting #621's URI test onto an engine frame; a live-URI byte budget on the
  command-text pattern). A ceiling question, theirs to reopen.
- **Zero governing records** for either path, including the row-owned `Arc` design.
- **Whether a *public* accessor should answer "same link?"** is open, and #635 made it live: the only
  outside observation of grouping is a decoded frame's frame-local `link_table` index. A consumer
  cannot ask in engine coordinates.
- **The widget's overlap rule has no engine-side counterpart** — OSC 8 wins over a detected URL only
  in `LinkController`.
- **A port answer's off-screen context is never checked.** The mirror holds the viewport only, so a
  line whose head sits above the top is kept on what the viewport shows of it; a wrong link needs
  identical on-screen text over a changed off-screen part.
- **One `lineAt` costs the backend a whole `viewport_logical_lines`**, since core has no one-row entry
  point (declined on #934 when no consumer called `lineAt`; PenTerm now does, #2101). The question
  count is bounded by motion; the cost per question is not.
- **A widget with a11y wired keeps two viewport mirrors**, each applying every frame.
- **Detection is viewport-only by construction.** A URL entirely in scrollback is never detected — a
  limitation no published document states.
