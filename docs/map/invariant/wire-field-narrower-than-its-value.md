# Invariant — a wire field is narrower than the value it carries, and only the producer can bound it

## The fact

The engine holds sizes and counts as `usize` / `isize`; the wire holds them as `u16`, `i16` or `u32`,
per field. `justerm-core/src/serialize.rs — encode` returns `Vec<u8>`, not a `Result` — it has no
channel to refuse — so a value that does not fit is narrowed silently by an `as` cast, and `decode`
returns `Ok` on the wrapped result. So **whatever produces the value must bound it before `encode`**:
not `encode` (cannot refuse), not `decode` (cannot tell a wrapped value from a legitimate one), not the
consumer (would be covering an upstream defect).

Which bounding strategy applies is a property of the field (deliberately not counted — a count here
once read "three" while listing four, then five):

- **Widen the field** when the value is genuinely unbounded — #621 moved every length prefix and
  viewport-scaled group count to `u32`.
- **Cap at a bound that costs nothing** when the value stops meaning anything past a limit — #661 caps
  a scroll `count` at its region's height, since a larger shift already moves every source row out.
- **Drop the item and assert in debug** when the value is a key that cannot be represented and
  narrowing would land on a live cell — #582's out-of-span group keys.
- **Take the group off the frame** when it should never have been per-frame state. Widening always
  works and is wrong here: #490 rejected widening the marker group counts because it would entrench
  what ADR-0020 records as an R3 violation ("a group must be `O(1)` or `O(viewport)`"; two such since
  its 2026-08-04 amendment), and v16 removed the absolute-line marker group.
- **Bound the producer at the field's own capacity** when an untrusted input allocates the value and no
  natural limit exists — #721 caps a buffer's live marker population at
  `justerm-core/src/term.rs — MAX_MARKERS` (`u16::MAX`). The 70 000th marker is meaningful; it is
  bounded because the wire field is the only limit anyone can name, the argument `MAX_COLUMNS` /
  `MAX_ROWS` (`term.rs`, `u16::MAX`) are written from — the old note placed `MAX_COLUMNS` in
  `serialize.rs`; it is in `term.rs` at HEAD 7bb31f7. Only against an untrusted entry point; against a
  trusted caller it is silent data loss. Measured before the geometry clamp: `Engine::new(70_000, 2)`
  built a 70 000-column grid whose frame declared `cols = 4464` and decoded `Ok`. No reference bounds a
  grid this way because none serializes one. A 4K display at a very small font is roughly 550 columns.

## Why it is cross-cutting

The narrowing lives in one file, so it reads as a wire-format concern — and that is why it keeps being
rediscovered elsewhere: the **bound** belongs to whichever territory produces the value, and those
producers (`Term::record_scroll` for damage, the marker allocator, the geometry clamp in
`Term::resize`, the overlay span builders) do not call each other or open the wire format. The author
of `ScrollOp::count`'s accumulator was not thinking about bytes, and the accumulator is where the defect
was.

Silent both ways: a wrapped `u16` count arrives small and the payload behind it is read as something
else; a wrapped `i16` arrives with the opposite sign, and the consumer executes the operation backwards.

## Territories it holds in

- [wire format](../territory/wire-format.md) — the casts, and the "`u32` iff viewport-bounded" rule. It
  owns the rule, not the values.
- [damage](../territory/damage.md) — `damage.rs — ScrollOp::count`, `isize` → `i16`, accumulated
  uncapped by `Term::record_scroll` and capped at the region height by `cap_scroll` inside
  `Term::scroll_delta` (#661).
- [frame](../territory/frame.md) — the header scalars and the span count (`u16`; one span per damaged
  line, so bounded by `MAX_ROWS`, exactly `u16::MAX`).
- [search](../territory/search.md) and [selection](../territory/selection.md) — overlay span group
  counts, which wrapped at 66 000 spans on a large viewport before #621 widened them.
- [marker](../territory/marker.md) — closed by #721 at the producer, not the field. 70 000 marks once
  encoded a declared count of 4 464 while writing 70 000 records; `decode` took the next group's count
  from the middle of record #4 465 and returned `Ok`, so every later group was garbage-derived. The
  viewport group's count (`term/markers.rs — Term::marker_positions`, row-filtered, unbounded because
  several marks share a line) is still `u16`, bounded by `MAX_MARKERS` — the only place the
  bound-the-producer strategy is load-bearing since v16 removed the absolute-line group (#490). Not
  reachable without an adversary: ordinary shell integration emits ≤ 4 marks per command over ≥ 1 line,
  so a default-scrollback session tops out near 40 000; the measured 70 000 needed a stream with no
  newline (#721).
- [decoration](../territory/decoration.md) — the underline-colour group's count, still `u16`, measured
  unreachable after #582 rather than fixed.

## What a violation looks like

Nothing throws; the symptom is downstream and looks like a consumer defect: a scroll executed in the
wrong direction or not at all (`count` past `i16::MAX`, or exactly a multiple of 65 536 arriving as
`0`); a group whose entries stop at an arbitrary index; a payload read against the wrong key.

The catching test is the round-trip identity on an **engine-produced** frame, `decode(encode(f)) == f`,
over an input class large enough to reach the bound. Hand-built `Frame` fixtures never reach it, which
is why each of these survived a suite full of round-trip tests.

## Discovery history

- **#621** (2026-07-29) — widened every length prefix and viewport-bounded count after a one-character
  search over a 1000×133 viewport wrapped an overlay count to 464 and decoded `Ok`; warned "do not
  assume those two are the only `as u16` narrowings".
- **#582** (2026-07-30) — `decode` accepted a span, key or scroll region not fitting the header; fixed
  placement and left `count` alone, since bounding it then would have rejected legitimate frames.
- **#661** (2026-07-31) — `ScrollOp::count`, which #621's warning did not reach (`i16`, fed by an
  accumulator): one 32 KB `feed()` of newlines encoded `32768` as `−32768`.
- **#490** — marker group off the frame (v16). **#721** — `MAX_MARKERS`.

#621 wrote the warning; #582 read the file and still left `count`; #661 found it from another direction.

## Where it will recur

- **Any new wire group or header scalar.** ADR-0020's rules gate whether a group belongs on the frame,
  not whether its count fits; ask the width question separately.
- **Any new accumulator** — `Term::record_scroll` is the shape: a value growing between acks, whose
  ceiling is the consumer's cadence rather than the grid.
- **Any change that raises a ceiling.** `MAX_ROWS` is `u16::MAX`, larger than `i16::MAX`; that gap is
  why capping the scroll count at region height was not sufficient alone.

Ask rather than trust a list (29 matches at HEAD 7bb31f7):

```sh
rg -n ' as (u8|u16|i16|u32)\)' justerm-core/src/serialize.rs
```
