# Invariant — a wire field is narrower than the value it carries, and only the producer can bound it

## The fact

The engine holds sizes and counts as `usize` / `isize`. The wire holds them as `u16`, `i16` or
`u32`, chosen per field. **`encode` returns `Vec<u8>`, not a `Result`** — it has no channel to
refuse — so a value that does not fit is narrowed silently by an `as` cast, and `decode` returns
`Ok` on the wrapped result. Nothing between the two can catch it.

Therefore: **whatever produces the value must bound it before it reaches `encode`.** Not `encode`,
which cannot refuse; not `decode`, which cannot tell a wrapped value from a legitimate one; and not
the consumer, which would be covering an upstream defect.

The bounding strategies in force are below, and which one applies is a property of the field rather
than a preference. **Deliberately not counted in this sentence** — it read "Three" while listing
four, then five, because a count is a status claim with nothing gating it (the same failure
`docs/map/README.md` records for roster tables, and #552 for a hand-copied roster):

- **Widen the field** when the value is genuinely unbounded — `#621` moved every length prefix and
  viewport-scaled group count to `u32`.
- **Cap at a bound that costs nothing** when the value stops meaning anything past a limit —
  `#661` caps a scroll `count` at its region's height, because a larger shift already moves every
  source row outside the region.
- **Drop the item and assert in debug** when the value is a *key* that cannot be represented and
  narrowing it would land on a live cell — `#582`'s out-of-span group keys.
- **Take the group off the frame entirely** when it should never have been per-frame state. This is
  the one that is easy to miss, because widening always *works* — and here it is the wrong answer:
  the two marker group counts overflow (below), and `#490` rejects widening them because doing so
  would entrench what ADR-0020 records as its **one stated R3 violation** (*"a group must be `O(1)`
  or `O(viewport)`"*). Ask which strategy the field's territory has already chosen before reaching
  for the obvious one.
- **Bound the producer at the field's own capacity** when the value is allocated by something
  untrusted and no natural limit exists — `#721` caps a buffer's live marker population at
  `MAX_MARKERS = u16::MAX`, so neither marker count can reach a value it cannot declare. Distinct
  from the second strategy above: a scroll `count` past its region height *stops meaning anything*,
  whereas the 70 000th marker is perfectly meaningful — it is bounded because the **wire field is
  the only limit anyone can name**, which is the argument `MAX_COLUMNS` is already written from
  (`serialize.rs`, `MAX_COLUMNS = u16::MAX` "for exactly that representational reason"). Reach for
  this one only when the producer is an untrusted entry point; against a trusted caller it is a
  silent data loss with no defect behind it.

## Why it is cross-cutting

The narrowing lives in one file (`serialize.rs`), so it reads as a wire-format concern — and that
is exactly why it keeps being rediscovered somewhere else. The **bound** belongs to whichever
territory produces the value, and a reader working in that territory has no reason to open the wire
format. `ScrollOp::count` is a [damage](../territory/damage.md) concept; the author who wrote its
accumulator was not thinking about bytes, and the accumulator is where the defect was.

The failure is also **silent in both directions**. A wrapped `u16` count arrives as a small number
and the payload behind it is read as something else; a wrapped `i16` arrives with the *opposite
sign*, so a consumer executes the operation backwards. Neither raises an error anywhere in the
stack, which is why this class survives to be found by a completeness pass rather than by a report.

## Territories it holds in

- [wire format](../territory/wire-format.md) — where the casts are, and where the *"`u32` iff
  viewport-bounded"* rule lives. It owns the rule; it does not own the values.
- [damage](../territory/damage.md) — `ScrollOp::count`, `isize` → `i16`. Capped at the region
  height by `Term::scroll_delta` (#661).
- [frame](../territory/frame.md) — the header's scalars, and the span count (`u16`; one span per
  damaged line, so bounded by `MAX_ROWS`, which is exactly `u16::MAX` — it fits with nothing to
  spare).
- [search](../territory/search.md) and [selection](../territory/selection.md) — the overlay span
  groups, whose counts wrapped at 66 000 spans on a large viewport before #621 widened them.
- [marker](../territory/marker.md) — **was this note's live instance; closed by #721 at the producer,
  not at the field.** Both marker group counts are `u16`, and 70 000 marks encoded a declared count
  of 4 464 while writing 70 000 records; `decode` then took the next group's count out of the middle
  of marker record #4 465 and returned `Ok`, so **every group after markers in that frame was
  garbage-derived**. The counts are unchanged and still `u16` — what changed is that `MAX_MARKERS`
  bounds a buffer's live population at `u16::MAX`, so the wrapping input can no longer exist (the
  fifth strategy above). Two corrections worth carrying, because this entry stated both wrongly:
  the groups do **not** both "count every live marker" — `markers` is row-filtered by
  `marker_positions` and is unbounded for the *other* reason, that several marks share a line; and
  it is **not** reachable without an adversary — ordinary shell integration emits ≤ 4 marks per
  command over ≥ 1 line each, so a default-scrollback session tops out near 40 000, and the measured
  70 000 needed a stream that never emits a newline (#721 measures both ends).
- [decoration](../territory/decoration.md) — the underline-colour group's count, still `u16`,
  measured unreachable after #582 rather than fixed.

## What a violation looks like

Nothing throws. The symptom is downstream and looks like a defect in the consumer:

- a scroll executed in the wrong direction, or not at all (`count` wrapping past `i16::MAX`, or
  landing exactly on a multiple of 65 536 and arriving as `0`);
- a group whose entries stop being read at an arbitrary index, because its count wrapped;
- a payload read against the wrong key, because the key was narrowed onto a live column.

The test that catches it is the round-trip identity on an **engine-produced** frame —
`decode(encode(f)) == f` — over an input class large enough to reach the bound. Hand-built `Frame`
fixtures never reach it, which is why every one of these survived a suite full of round-trip tests.

## Discovery history

Three separate discoveries, none of which reached the next:

- **#582** (2026-07-30) — `decode` accepted a span, a group key or a scroll region that does not fit
  the frame's own header. Fixed the *placement* question and explicitly left `count` alone, because
  at the time bounding it would have rejected frames the engine legitimately produced.
- **#621** (2026-07-29) — widened every length prefix and viewport-bounded group count to `u32`,
  after a one-character search over a 1000×133 viewport wrapped an overlay count to 464 and decoded
  `Ok`. Its own acceptance item warned *"do not assume those two are the only `as u16` narrowings"*.
- **#661** (2026-07-31) — `ScrollOp::count`, which that warning did not reach: it is `i16` rather
  than `u16`, and it is fed by an accumulator rather than by a length. A single 32 KB `feed()` of
  newlines encoded `32768` as `−32768`.

The ordering is the argument for this note existing. #621 wrote the warning; #582 read the file it
warned about and still left `count`; #661 found `count` from a different direction entirely.

## Where it will recur

- **Any new wire group or header scalar.** ADR-0020's three rules gate whether a group *belongs* on
  the frame; they say nothing about whether its count fits. Ask the width question separately.
- **Any new accumulator.** `Term::record_scroll` is the shape to look for — a value that grows
  between acks rather than being derived from the buffer, so its ceiling is the consumer's cadence
  rather than the grid.
- **Any change that raises a ceiling.** `MAX_ROWS` is `u16::MAX`, which is *larger* than
  `i16::MAX`; that gap is why capping the scroll count at its region's height was not sufficient on
  its own.

Ask, rather than trusting a list here:

```sh
rg -n ' as (u8|u16|i16|u32)\)' justerm-core/src/serialize.rs
```
