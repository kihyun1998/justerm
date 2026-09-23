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

/// Which byte ended an OSC sequence — and therefore which one ends its reply.
///
/// The engine relays this rather than choosing: a query event carries the
/// terminator the request arrived with, and the consumer hands it back to the
/// matching `report_*`. Under [ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md) the parse-time fact is a *mechanism* only
/// the engine can observe, while *which* terminator to send is policy — and a
/// consumer cannot exercise a policy on a fact it was never given.
///
/// The terminator rides the event rather than being remembered by the engine, because
/// [`crate::Engine::drain_events`] hands over a batch: a consumer may hold two queries at once
/// and answer them in either order. This follows xterm's documented behaviour — *"when
/// returning information, uses the same terminator used in a query"* (`ctlseqs.txt`).
///
/// **Exhaustive on purpose ([#843](https://github.com/kihyun1998/justerm/issues/843)'s rule).**
/// The 8-bit C1 `ST` (`0x9C`) is not a third member because [`crate::Engine::feed`] does not
/// treat a lone `0x80..=0x9F` byte as a control at all; were that contract revisited, the member
/// would follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Terminator {
    /// `ESC \` (ST), the terminator ECMA-48 documents and xterm prefers.
    ///
    /// The default, and what an OSC ended by **any other byte that ends one**
    /// resolves to. That stream is real rather than theoretical: `vte` ends a
    /// string on three byte classes — `BEL`, the cancel pair `CAN`/`SUB` (`0x18` /
    /// `0x1a`), and a bare `ESC` opening the next sequence — and only the first is
    /// reported as bell-terminated. A query ended by a bare `ESC` is relayed and
    /// answered ST; one ended by `CAN`/`SUB` is cancelled, as in xterm, and never
    /// reaches the consumer.
    ///
    /// **Read "any other byte that ends one" strictly: the 8-bit C1 `ST` (`0x9C`) is
    /// not a fourth class.** It does not end the string, so there is no event
    /// to carry a terminator and nothing resolves to this variant — the OSC stays
    /// open instead. See [`crate::Engine::feed`] for why that is a contract.
    ///
    /// That is the right answer, not a fallback — xterm answers the same shape with ST.
    #[default]
    St,
    /// `BEL` (`0x07`), supported for legacy applications.
    ///
    /// Real applications still emit it: `nvim` 0.8.0 asks `ESC ] 11 ; ? BEL` in
    /// this crate's own `cursor_color_nvim.raw` fixture. And the shell idiom
    /// `printf '\e]11;?\a'; read -d $'\a'` reads *until* BEL, so an ST answer to
    /// a BEL question blocks that `read` until it times out.
    Bel,
}

impl Terminator {
    /// The bytes that end a reply carrying this terminator.
    pub(crate) fn bytes(self) -> &'static [u8] {
        match self {
            Terminator::St => b"\x1b\\",
            Terminator::Bel => b"\x07",
        }
    }
}

/// Which selection an `OSC 52` clipboard request names.
///
/// A *value*, never the protocol byte, so a consumer never parses the sequence —
/// the same reason [`TermEvent::SetPaletteColor`] carries a `u8` index rather
/// than the field it was written in.
///
/// **Three members, and `p` is kept apart from `s`**, so the value a consumer hands back to
/// `report_clipboard` names the selector the application wrote. The sequence's other selectors
/// (`q` and the eight cut buffers) are ignored rather than folded onto a neighbour.
///
/// **`#[non_exhaustive]` ([#843](https://github.com/kihyun1998/justerm/issues/843)).** `q` and the
/// cut buffers are unmodelled, so a later slice may name one. A consumer meeting a member it does
/// not know can decline the request, which is already how it refuses any of them.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipboardTarget {
    /// The system clipboard — the `c` field, **and the empty field**.
    ///
    /// The empty field is the common form in the wild rather than an edge case:
    /// it is what `tmux` 3.2a was measured emitting for both an ordinary
    /// copy-mode copy and `set-buffer -w`. Reading it as "unrecognised"
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
    /// (`button.c:2081`), and under [ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md) a setting is the consumer's. So the
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
    /// rest of the tail.
    Selection,
}

/// Which notification sequence carried a [`TermEvent::Notification`].
///
/// `#[non_exhaustive]`: other notification protocols exist (kitty's `OSC 99`) and a
/// later version may relay one.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationSequence {
    /// `OSC 9` — iTerm2's notification, and ConEmu's numbered subcommands (`4`
    /// progress, `9` working directory, …).
    Osc9,
    /// `OSC 777` — rxvt-unicode's extension dispatch; `notify ; title ; body` is its
    /// notification.
    Osc777,
}

/// A consumer-facing event emitted while parsing the VT stream.
///
/// **`#[non_exhaustive]`, so a consumer must carry a `_` arm and a new variant
/// never breaks one.** Ignoring an event you do not recognise is safe on this channel; new
/// variants are announced in the release notes rather than by the compiler.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// The window title is now this string.
    ///
    /// Read the tense carefully: this is **not** only "the
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
    ///
    /// A title containing 15 or more `;` arrives cut short, because the parser this
    /// engine builds on passes at most 16 OSC fields; the shorter title is not marked.
    Title(String),
    /// The terminal bell rang (BEL, `0x07`).
    Bell,
    /// The working directory was reported (OSC 7), e.g. `file://host/path`.
    ///
    /// Passed as declared, except that a value containing 15 or more unencoded `;`
    /// arrives cut short (the same 16-field parser bound as [`TermEvent::Title`]).
    /// An emitter that percent-encodes `;` never reaches it.
    Cwd(String),
    /// The application sent a notification sequence (`OSC 9` or `OSC 777`).
    ///
    /// `payload` is everything between the code's `;` and the terminator: never split,
    /// parsed or unescaped, though C0 controls inside it never reach it (the parser drops
    /// them within an OSC string). Reading it is the consumer's, and **not every payload
    /// on these codes is a notification**:
    ///
    /// - `OSC 9` is iTerm2's free-text notification, and also ConEmu's family of
    ///   numbered subcommands — a leading `n ;` — among them `4` (progress) and `9`
    ///   (the working directory, which a shell may send on every prompt and which does
    ///   not arrive as [`TermEvent::Cwd`]). A payload starting with a known subcommand
    ///   is that subcommand, not text to show.
    /// - `OSC 777` is rxvt-unicode's extension dispatch. Only a first field of `notify`
    ///   (`notify ; title ; body`, whose body may be JSON) is a notification; other
    ///   extension names ride the same code.
    ///
    /// **`maybe_truncated` means "cannot be confirmed whole", not "was cut".** The
    /// parser this engine builds on passes at most 16 OSC fields and drops the rest,
    /// and a sequence cut there is indistinguishable from one with exactly 16 fields.
    /// So the flag is set whenever all 16 arrived, which is a payload holding 14 or
    /// more `;`: at exactly 14 it is complete, and from 15 it is a prefix of what was
    /// sent. When the flag is clear, the payload is whole.
    Notification {
        sequence: NotificationSequence,
        payload: String,
        maybe_truncated: bool,
    },
    /// The app requested 80/132-column mode (DECCOLM `?3`). justerm is
    /// dimension-free, so this is a *request* — the consumer may honor it by
    /// calling `resize(cols, rows)`, or ignore it. `cols` is 80 or 132.
    ColumnMode { cols: usize },
    /// The app queried the light/dark color scheme (DSR `CSI ? 996 n`). justerm
    /// is theme-agnostic, so the consumer (which knows the scheme) answers by
    /// calling `Engine::report_color_scheme`.
    ColorSchemeQuery,
    /// The app set ANSI palette entry `index` to `spec` (OSC 4). One event per
    /// `index ; spec` pair the engine accepts — a pair whose index does not parse
    /// as a `u8`, or whose spec is `?` (a query) or **empty**, produces
    /// none, and the pairs around it are unaffected either way. The cell still
    /// references `Indexed(index)` — only the consumer's `palette[index]` changes,
    /// so the engine stays theme-agnostic.
    ///
    /// **`spec` is never empty**, so a consumer's colour parser is never handed a
    /// blank string. It is otherwise verbatim and unvalidated: the engine holds no
    /// palette and parses no colour, so `spec` may still be whitespace or
    /// nonsense, and interpreting it is the consumer's ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)).
    SetPaletteColor { index: u8, spec: String },
    /// The app set the default foreground colour (OSC 10). Raw spec, forwarded
    /// for the consumer to apply — theme-agnostic, like [`SetBackground`](Self::SetBackground).
    SetForeground(String),
    /// The app set the default background colour (OSC 11). The engine is
    /// theme-agnostic, so it forwards the raw spec string (`rgb:…`/`#…`) for the
    /// consumer to parse and apply to its palette — it never holds hex.
    SetBackground(String),
    /// The app reset palette entries to the theme default (OSC 104). `None` =
    /// the whole table (no argument); `Some(index)` = one entry, one event per
    /// index given. The consumer restores its palette.
    ResetPaletteColor(Option<u8>),
    /// The app queried ANSI palette entry `index` (OSC 4 with `?` for that pair);
    /// the consumer answers with `report_palette_color`.
    QueryPaletteColor {
        index: u8,
        /// The terminator `report_palette_color` must answer with.
        terminator: Terminator,
    },
    /// The app set the cursor colour (OSC 12). The third slot of the same
    /// dynamic-colour sequence `SetForeground` and `SetBackground` ride, and
    /// theme-agnostic for the same reason: the raw spec is forwarded and the
    /// consumer — which owns the palette *and* the cursor's contrast guard —
    /// applies it.
    SetCursorColor(String),
    /// The app queried the cursor colour (OSC 12 with `?`); the consumer answers
    /// with `report_cursor_color`.
    QueryCursorColor {
        /// The terminator `report_cursor_color` must answer with.
        terminator: Terminator,
    },
    /// The app reset the cursor colour to the theme default (OSC 112). The
    /// third member of the 110/111/112 reset family, and the one real
    /// applications emit most: `nvim` sends it on startup, on every alt-screen
    /// transition and on exit.
    ResetCursorColor,
    /// The app reset the default foreground to the theme default (OSC 110).
    ResetForeground,
    /// The app reset the default background to the theme default (OSC 111).
    ResetBackground,
    /// The app queried the default foreground colour (OSC 10 with `?`); the
    /// consumer answers with `report_foreground`.
    QueryForeground {
        /// The terminator `report_foreground` must answer with.
        terminator: Terminator,
    },
    /// The app queried the default background colour (OSC 11 with `?`). The
    /// theme-agnostic engine relays it; the consumer answers with
    /// `report_background`, mirroring `ColorSchemeQuery`.
    QueryBackground {
        /// The terminator `report_background` must answer with.
        terminator: Terminator,
    },
    /// The app asked for `text` to be put on `target` (`OSC 52` with a payload).
    /// The engine has already base64-decoded it, and holds no clipboard
    /// of its own.
    ///
    /// **This is a request, not a fact.** Whether the copy happens is the
    /// consumer's: it owns the platform clipboard, any permission model and any
    /// prompt, and a consumer that drops this event has refused the copy. The
    /// engine carries no allow/deny knob — under
    /// [ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
    /// that gate is the consumer's.
    ///
    /// **An empty `text` means "clear it"**, as the spec (`Pd` *"becomes the new selection"*
    /// whatever it is) and xterm end that exchange: an empty payload is a well-formed encoding of
    /// no bytes, so it reaches the consumer through the ordinary path.
    ClipboardStore {
        target: ClipboardTarget,
        text: String,
    },
    /// The app asked what is on `target` (`OSC 52` with a `?` payload).
    /// The consumer answers by calling `report_clipboard`, which encodes the
    /// reply — or declines, which is how a clipboard *read* is refused
    /// independently of a write.
    ///
    /// The engine cannot answer this itself and deliberately holds nothing that
    /// would let it: a query is answered from the consumer's clipboard or not at
    /// all, so there is no engine state here for a hostile application to read
    /// back. Same `Query…` + `report_…` shape as `OSC 4`/`10`/`11`/`12`.
    QueryClipboard {
        target: ClipboardTarget,
        /// The terminator `report_clipboard` must answer with.
        terminator: Terminator,
    },
    /// A decoration marker's line left the buffer — evicted past the scrollback
    /// cap, or scrolled out of an in-screen region. The handle is now
    /// dead; the consumer drops the decoration bound to it. This is the
    /// frame-mode equivalent of xterm's `IMarker.onDispose` — disposal is a
    /// point-in-time fact (a marker absent from a frame may merely be scrolled
    /// off-screen), so it rides the event queue, not the frame overlay.
    MarkerDisposed(MarkerId),
    /// A marker was created — by `add_marker`, or by the *stream* through an
    /// OSC 133 command mark, which the consumer never called for.
    ///
    /// The mirror of [`TermEvent::MarkerDisposed`], and it exists for the same reason
    /// [ADR-0020](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0020-what-qualifies-for-the-frame-snapshot.md) R1 gives: an appearance is an occurrence, not state, so it rides this
    /// queue rather than a frame field. Without it a consumer that pulled a marker
    /// index (`Engine::marker_index`) has no way to learn of a marker born after its
    /// pull — the population would only ever shrink.
    ///
    /// `line` is absolute at the moment of creation, and `evicted_total` / `epoch` are the
    /// instant it is absolute at — the same triple [`crate::MarkerIndex`] carries, because
    /// this event is that pull's incremental mirror. The consumer appends the entry with
    /// the basis it arrived on and rebases it exactly like a pulled one.
    ///
    /// **The three are one fact and none is usable alone.** A single `feed` can create a
    /// marker and then evict, so `Frame::evicted_total` at the end of the batch is not the basis
    /// `line` is absolute on; and a reflow or region rotate moves markers individually, which
    /// only `epoch` dates.
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
        /// the moment of creation. Two lines dated with different epochs are
        /// answers about different buffers and nothing rebases one onto the other, so a
        /// consumer adopts this entry only into the generation it names and lets the
        /// re-pull that the bump already forces supply it otherwise.
        ///
        /// Compare it for **equality**, never for order: the counter is
        /// `wrapping_add`, so `<` is meaningless across a wrap while `==` is exact.
        epoch: u32,
    },
}
