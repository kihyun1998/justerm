//! Consumer event surface (#12): point-in-time notifications the engine
//! accumulates while parsing, for the consumer to drain.
//!
//! Pull, not push — the engine queues events during `feed` and the consumer
//! takes them with `drain_events`, mirroring the rest of the pull cadence
//! (`damage` / `frame` / `reset_damage`). No callback is injected across the
//! boundary, so the engine stays decoupled from the consumer's event loop
//! (unlike alacritty's `EventListener`, whose push model would couple them).
//!
//! OSC 8 hyperlinks are deliberately absent — a hyperlink is per-cell state
//! (which cells are links), not a point-in-time event, so it is modelled like
//! graphemes in its own slice (#26), not here.

use crate::serialize::{MarkerId, MarkerKind};

/// Which selection an `OSC 52` clipboard request names (#828).
///
/// A *value*, never the protocol byte, so a consumer never parses the sequence —
/// the same reason [`TermEvent::SetPaletteColor`] carries a `u8` index rather
/// than the field it was written in.
///
/// **Three members, and `p` is kept apart from `s` deliberately.** The
/// sequence's target field admits `c`, `p`, `q`, `s` and the eight cut buffers
/// (`ctlseqs.txt:2156`); justerm models the three a consumer can act on and
/// ignores the rest rather than folding them onto a neighbour, since mapping `q`
/// onto a selection would be the engine inventing an equivalence the application
/// did not ask for. ghostty folds every unrecognised kind onto the clipboard
/// (`src/termio/stream_handler.zig:1013`); alacritty ignores them
/// (`alacritty_terminal/src/term/mod.rs:1714`), and so does this. Read
/// ghostty's from the `switch` and not from the comment four lines above it,
/// which says *"we ignore the 'kind' field and always use the standard
/// clipboard"* and is contradicted by the code under it — only the `else` arm
/// goes to `.standard`.
///
/// **The first draft collapsed `p` and `s` into one member, and that was wrong
/// on the wire.** alacritty collapses them
/// (`alacritty_terminal/src/term/mod.rs:1713`) but replies with the byte the
/// application sent (`:1744`), so the collapse never reaches a client. This
/// engine hands the consumer a value and gets it back at `report_clipboard`, so
/// a collapse here would answer `ESC ] 52 ; s ; ?` naming `p` — a selector the
/// application never wrote, in the one field a client could pair a reply on. The
/// spec lists the two separately (`ctlseqs.txt:2157`), xterm binds them to
/// different atoms (`misc.c:3327`) and echoes the recognised list back
/// (`misc.c:3384`), and ghostty keeps three locations apart in both directions
/// (`src/Surface.zig:5954`, `src/terminal/c/terminal.zig:2942`). Splitting the
/// member is what lets the value round-trip without the engine remembering
/// anything.
///
/// **`#[non_exhaustive]` (#843).** The set is open by the paragraph above: `q` and
/// the eight cut buffers are in the sequence and unmodelled here, so a later slice
/// may name one. A consumer meeting a member it does not know can decline the
/// request, which is already how it refuses any of them.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardTarget {
    /// The system clipboard — the `c` field, **and the empty field**.
    ///
    /// The empty field is the common form in the wild rather than an edge case:
    /// it is what `tmux` 3.2a was measured emitting for both an ordinary
    /// copy-mode copy and `set-buffer -w` (#828). Reading it as "unrecognised"
    /// would drop the only emission this project has observed.
    Clipboard,
    /// The primary selection — the `p` field. On a platform with no primary
    /// selection a consumer may treat it as [`Clipboard`](Self::Clipboard); the
    /// engine does not make that choice for it.
    Primary,
    /// The `s` field — *"the configurable primary/clipboard selection"*
    /// (`ctlseqs.txt:2161`), which is to say: whichever of the two the user has
    /// configured.
    ///
    /// **Relayed rather than resolved, and that is the boundary working.** The
    /// thing that decides what `s` means is a setting: xterm resolves `SELECT`
    /// through `DefaultSelection`, which is the `selectToClipboard` resource
    /// (`button.c:2081`), and under ADR-0017 a setting is the consumer's. So the
    /// application's choice is carried through unchanged and the consumer
    /// resolves it against the configuration it owns.
    ///
    /// **A consumer with no such setting should treat this as
    /// [`Primary`](Self::Primary), because that is what xterm-as-shipped does**
    /// — `selectToClipboard` defaults to false. Worth stating rather than left
    /// to taste, since the alternative reading sends the copy somewhere the
    /// reference would not.
    ///
    /// And the setting is not purely out of reach: **DECSET 1041 sets the same
    /// resource from the stream** (`ctlseqs.txt:1008`), so an engine that
    /// tracked that mode could resolve `s` itself. justerm does not model 1041,
    /// which is a *declined* capability rather than an impossible one — the
    /// honest form of the claim, and the mode is unimplemented here like the
    /// rest of the tail (#47).
    Selection,
}

/// A consumer-facing event emitted while parsing the VT stream.
///
/// **`#[non_exhaustive]`, so a consumer must carry a `_` arm and a new variant
/// never breaks one.** Decided 2026-09-02, by the maintainer, while #828 was
/// adding two — and what decided it was neither this slice nor any consumer we
/// can see.
///
/// **What decided it is `CLAUDE.md`'s own identity statement**: *"`justerm-core`
/// is not penterm-only — it is a reusable, independent crate."* That sentence
/// says there are consumers we cannot edit, which is precisely what this
/// attribute defends; a crate whose identity were "internal, used by penterm"
/// would want the opposite, because there a broken build is the compiler doing
/// us a favour. So this follows from a call already made rather than from a
/// preference, and it reverses only if that identity does.
///
/// Three measurements, so the next reader does not have to retake them:
///
/// - **crates.io reverse dependencies: zero** (the single row the API returns is
///   this crate itself), across 248 downloads split over 11 versions — i.e. no
///   external consumer exists *today*. That is why the identity statement had to
///   decide it: there was nothing to observe.
/// - **Cost in this workspace: zero.** `cargo test --workspace` (87 suites) and
///   `clippy -D warnings` both stay green. A same-crate `match` may still be
///   exhaustive, `justerm-wasm-decode` and `justerm-renderer` never name
///   `TermEvent`, and `justerm-web`'s `events.ts` is a deliberately narrower
///   union (title / bell / cwd).
/// - **The window closes at `1.0.0`.** Adding this is free while the crate is
///   `0.x` and is *itself* a breaking change afterwards, while an enum without
///   it turns every future variant into a major bump. Conformance here is
///   cumulative by design (#47 is a perpetual tail) and the two slices before
///   this one added three variants and two, so that rate is measured rather than
///   assumed.
///
/// **The argument that lost, recorded because it is a real cost.** An exhaustive
/// match is a *feature* for a consumer: the compiler tells them a new event
/// exists and makes them decide about it. penterm's `route_event` is the worked
/// example — its `ColumnMode` and `ColorSchemeQuery` arms carry a comment
/// explaining why each is dropped, written by someone the compiler had just
/// informed. That signal is given up here, and it now has to come from release
/// notes. It loses because this is a *notification* channel where ignoring an
/// unknown event is documented as safe, so the guarantee belongs in prose rather
/// than in the type — but a consumer who wanted the old behaviour is not
/// imagining the loss.
///
/// (penterm was the evidence that an outside exhaustive matcher can exist — its
/// five-variant `match` predates nine minor versions of this enum — and not the
/// reason. It is being reimplemented, which is exactly why it could not be.)
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// The window title is now this string.
    ///
    /// Read the tense carefully: since #823 this is **not** only "the
    /// application set a title". Two paths emit it — `OSC 0`/`OSC 2`, and an
    /// XTWINOPS title *pop* (`CSI 23 t`) restoring what an earlier `CSI 22 t`
    /// saved. A consumer that treats it as "the title is now this" is correct
    /// for both; one that treats it as "the application just chose this" is
    /// wrong for the second, which is why there is no separate pop event.
    ///
    /// A pop can legitimately restore the **empty** string — every application
    /// measured pushes at startup, before setting a title of its own — and that
    /// means "go back to whatever you would show by default", not "show a blank
    /// title".
    Title(String),
    /// The terminal bell rang (BEL, `0x07`).
    Bell,
    /// The working directory was reported (OSC 7), e.g. `file://host/path`.
    Cwd(String),
    /// The app requested 80/132-column mode (DECCOLM `?3`). justerm is
    /// dimension-free, so this is a *request* — the consumer may honor it by
    /// calling `resize(cols, rows)`, or ignore it. `cols` is 80 or 132 (#82).
    ColumnMode { cols: usize },
    /// The app queried the light/dark color scheme (DSR `CSI ? 996 n`). justerm
    /// is theme-agnostic, so the consumer (which knows the scheme) answers by
    /// calling `Engine::report_color_scheme` (#85).
    ColorSchemeQuery,
    /// The app set ANSI palette entry `index` to `spec` (OSC 4). One event per
    /// `index ; spec` pair in the sequence. The cell still references
    /// `Indexed(index)` — only the consumer's `palette[index]` changes, so the
    /// engine stays theme-agnostic (#122).
    SetPaletteColor { index: u8, spec: String },
    /// The app set the default foreground colour (OSC 10). Raw spec, forwarded
    /// for the consumer to apply — theme-agnostic, like [`SetBackground`](Self::SetBackground) (#122).
    SetForeground(String),
    /// The app set the default background colour (OSC 11). The engine is
    /// theme-agnostic, so it forwards the raw spec string (`rgb:…`/`#…`) for the
    /// consumer to parse and apply to its palette — it never holds hex (#122).
    SetBackground(String),
    /// The app reset palette entries to the theme default (OSC 104). `None` =
    /// the whole table (no argument); `Some(index)` = one entry, one event per
    /// index given. The consumer restores its palette (#122).
    ResetPaletteColor(Option<u8>),
    /// The app queried ANSI palette entry `index` (OSC 4 with `?` for that pair);
    /// the consumer answers with `report_palette_color` (#122).
    QueryPaletteColor { index: u8 },
    /// The app set the cursor colour (OSC 12, #832). The third slot of the same
    /// dynamic-colour sequence `SetForeground` and `SetBackground` ride, and
    /// theme-agnostic for the same reason: the raw spec is forwarded and the
    /// consumer — which owns the palette *and* the cursor's contrast guard —
    /// applies it.
    SetCursorColor(String),
    /// The app queried the cursor colour (OSC 12 with `?`); the consumer answers
    /// with `report_cursor_color` (#832).
    QueryCursorColor,
    /// The app reset the cursor colour to the theme default (OSC 112, #832). The
    /// third member of the 110/111/112 reset family, and the one real
    /// applications emit most: `nvim` sends it on startup, on every alt-screen
    /// transition and on exit.
    ResetCursorColor,
    /// The app reset the default foreground to the theme default (OSC 110, #122).
    ResetForeground,
    /// The app reset the default background to the theme default (OSC 111, #122).
    ResetBackground,
    /// The app queried the default foreground colour (OSC 10 with `?`); the
    /// consumer answers with `report_foreground` (#122).
    QueryForeground,
    /// The app queried the default background colour (OSC 11 with `?`). The
    /// theme-agnostic engine relays it; the consumer answers with
    /// `report_background` (#122), mirroring `ColorSchemeQuery`.
    QueryBackground,
    /// The app asked for `text` to be put on `target` (`OSC 52` with a payload,
    /// #828). The engine has already base64-decoded it, and holds no clipboard
    /// of its own.
    ///
    /// **This is a request, not a fact.** Whether the copy happens is the
    /// consumer's: it owns the platform clipboard, any permission model and any
    /// prompt, and a consumer that drops this event has refused the copy. The
    /// engine carries no allow/deny knob, which is where it parts company with
    /// alacritty — alacritty gates the same sequence behind a four-state config
    /// (`alacritty_terminal/src/term/mod.rs:1706`) because alacritty *is* the
    /// consumer. Under ADR-0017 that gate lives one layer out.
    ///
    /// **An empty `text` means "clear it".** `ESC ] 52 ; c ; ESC \` carries a
    /// payload that decodes to nothing, and both the spec and xterm end that
    /// exchange with an empty selection — the spec because `Pd` *"becomes the
    /// new selection"* whatever it is (`ctlseqs.txt:2166`), xterm because it
    /// clears the buffer before appending anything (`misc.c:3410`). Note this is
    /// **not** the spec's *"neither a base64 string nor `?`"* clause at
    /// `ctlseqs.txt:2174`, which the engine diverges from: an empty payload is a
    /// perfectly well-formed encoding of no bytes, so it never reaches that
    /// sentence. Citing `:2174` here, as an earlier draft did, would have the
    /// same line standing as authority followed and as authority departed from.
    /// ghostty pins the same input under a test named *"clear clipboard"*
    /// (`src/terminal/osc/parsers/clipboard_operation.zig:93`). It needs no rule
    /// of its own here, which is the argument for having none: an empty payload
    /// *is* a well-formed encoding of no bytes, so it reaches the consumer
    /// through the ordinary path.
    ClipboardStore {
        target: ClipboardTarget,
        text: String,
    },
    /// The app asked what is on `target` (`OSC 52` with a `?` payload, #828).
    /// The consumer answers by calling `report_clipboard`, which encodes the
    /// reply — or declines, which is how a clipboard *read* is refused
    /// independently of a write.
    ///
    /// The engine cannot answer this itself and deliberately holds nothing that
    /// would let it: a query is answered from the consumer's clipboard or not at
    /// all, so there is no engine state here for a hostile application to read
    /// back. Same `Query…` + `report_…` shape as `OSC 4`/`10`/`11`/`12`.
    QueryClipboard { target: ClipboardTarget },
    /// A decoration marker's line left the buffer — evicted past the scrollback
    /// cap, or scrolled out of an in-screen region (#118). The handle is now
    /// dead; the consumer drops the decoration bound to it. This is the
    /// frame-mode equivalent of xterm's `IMarker.onDispose` — disposal is a
    /// point-in-time fact (a marker absent from a frame may merely be scrolled
    /// off-screen), so it rides the event queue, not the frame overlay.
    MarkerDisposed(MarkerId),
    /// A marker was created (#490) — by `add_marker`, or by the *stream* through an
    /// OSC 133 command mark, which the consumer never called for.
    ///
    /// The mirror of [`TermEvent::MarkerDisposed`], and it exists for the same reason
    /// ADR-0020 R1 gives: an appearance is an occurrence, not state, so it rides this
    /// queue rather than a frame field. Without it a consumer that pulled a marker
    /// index (`Engine::marker_index`) has no way to learn of a marker born after its
    /// pull — the population would only ever shrink.
    ///
    /// `line` is absolute at the moment of creation, and `evicted_total` / `epoch` are the
    /// instant it is absolute at — the same triple [`crate::MarkerIndex`] carries, because
    /// this event is that pull's incremental mirror. The consumer appends the entry with
    /// the basis it arrived on and rebases it exactly like a pulled one.
    ///
    /// **The two are one fact and neither is usable alone (#737).** A single `feed` can
    /// create a marker and then evict, so by the end of the batch the buffer's origin has
    /// moved out from under the line this event already carries. `Frame::evicted_total` is
    /// the basis at the *end* of that batch, so reading `line` against it misplaces the
    /// marker by however much the batch evicted after the birth — measured at three lines,
    /// with the event line, both frame bases, the epoch and `Frame::marker_count` all
    /// identical to the batch that evicted *first* and needs no adjustment at all.
    ///
    /// **And a basis dates only a uniform move (#741).** Eviction shifts every marker by
    /// the same amount, which is what one scalar can say; a reflow or a region rotate
    /// moves them *individually*, which is what `epoch` is for. A birth still queued when
    /// the epoch moves describes a buffer that no longer exists, and carrying only the
    /// basis leaves that indistinguishable from a birth in the current generation —
    /// measured, a mark at absolute 3 reflowed to 5 with the basis unmoved at 0.
    ///
    /// Deliberately not an epoch bump: a bump costs a whole re-pull, and creation is
    /// `O(1)` information.
    MarkerCreated {
        id: MarkerId,
        line: u32,
        kind: MarkerKind,
        /// Lines evicted since RIS at the moment of creation — the basis `line` is
        /// absolute at. Carried rather than inferred so that placement does not depend on
        /// whether the consumer drains this queue before or after it reads the frame,
        /// which nothing in the API specifies.
        ///
        /// It is the same quantity `Frame::evicted_total` reports, so a consumer whose
        /// transport crosses a language boundary owes it the same treatment: the wasm
        /// frame getter hands its `u64` over as an `f64` deliberately (exact to 2^53),
        /// because a `BigInt` on one side of a subtraction and a `number` on the other is
        /// a `TypeError`, not a rounding question.
        evicted_total: u64,
        /// The marker generation this line belongs to — [`crate::MarkerIndex::epoch`] at
        /// the moment of creation (#741). Two lines dated with different epochs are
        /// answers about different buffers and nothing rebases one onto the other, so a
        /// consumer adopts this entry only into the generation it names and lets the
        /// re-pull that the bump already forces supply it otherwise.
        ///
        /// Compare it for **equality**, never for order: the counter is
        /// `wrapping_add`, so `<` is meaningless across a wrap while `==` is exact.
        epoch: u32,
    },
}
