# Territory — hyperlinks

## What it is

Two unrelated ways a cell becomes clickable, deliberately kept apart:

- **OSC 8** — the application *declares* a link, so the engine stores which cells carry which URI
- **Plain-text URL detection** — nobody declared anything, so the consumer runs a regex over
  assembled text and decides

The first is per-cell state that survives in the buffer; the second is a policy applied to a snapshot.
They share a name and almost nothing else.

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  the split above *is* this record applied twice: storing declared links needs the buffer, so it is
  core; deciding what counts as a URL is policy, so it is not. Core has no regex dependency as a
  result
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
  re-validated. Whether a cell can be both is answered by nothing.

## Code

- `justerm-core/src/cell.rs` — `LINK_PRESENT`
- `justerm-core/src/grid.rs` — `Links`, the row's column-keyed link map
- `justerm-core/src/term.rs` — `Term::current_link` and the per-frame remap into `link_table`
- `justerm-core/src/serialize.rs` — `Frame`'s `link_table`
- `justerm-web/src/links.ts` — the URL regex and validation policy, over
  [logical lines](logical-lines.md)

## Reference behaviour

[OSC 8 hyperlinks — where the URI lives, and what frees it](../../agents/reference-facts.md#osc-8-hyperlinks--where-the-uri-lives-and-what-frees-it-628635-verified-2026-07-30)
— added by #628, which is when the area first needed them. Two questions with **different** answers:
all three references free the storage (3:0, and justerm was the outlier until #628), while only
xterm.js groups by `id=` (1:2, so #635 is a conformance gap against one reference, not a consensus).

Read the section's own warning before citing it: taking only the first row of each reference gives
*"they all keep a registry"*, which is how #46 arrived at a permanent pool — the reclamation half
does not appear at the site where the id is minted.

Still unpinned: `LINK_PRESENT` is described as occupying xterm's `HAS_EXTENDED` slot — a concrete
claim about another implementation's bit layout, in a comment, with no row.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the link map is one of the three, under
  the same presence-bit discipline: read only through the gate, ride with the row, and a write that
  clears the cell owes the bit rather than the map

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
- **`id=` grouping is not honoured** (#635): an application saying "these two runs are the same link"
  is not heard, so a link split across lines is two links to the consumer. Deferred at #26 and
  recorded in `architecture.md` as "a later refinement"; the other unported half of the same xterm
  function that #628 finished.
- **Nothing states whether the two paths may overlap.** A cell inside a declared OSC 8 link is also
  text that the URL regex will match; which one a click follows is undefined by every artifact.
- **The `HAS_EXTENDED` claim is unpinned**, and it is a statement about another project's bit layout.
- **Detection is viewport-only by construction.** A URL entirely in scrollback is never detected,
  because the consumer only ever sees assembled *viewport* lines — a limitation no document states.
