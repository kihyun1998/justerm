//! The decoration-marker surface: engine-owned marks bound to absolute buffer lines,
//! the OSC 133 semantic-command queries built on them, and the fixups that keep a mark
//! on its content — three for the buffer moving under it, and one for the content dying
//! where it stands.
//!
//! **That second kind arrived late and is the shape to remember (#750).** The three
//! movers below repair a *coordinate*, and for a long time they read as the whole job,
//! because every way a mark could stop describing its content moved the buffer. An
//! in-place erase does not: the row stays exactly where it is while everything the mark
//! was about stops existing, so no verb fired, no `MarkerDisposed` was announced, and
//! `command_lines` answered with commands that were not there. `dispose_markers_on_row`
//! is the repair for that class, and the two are not interchangeable — a mover cannot
//! see a destruction and a destroyer cannot see a move.
//!
//! A marker is the same kind of thing as a selection anchor — an absolute
//! `[scrollback ++ screen]` line index that survives an ordinary scroll and has to be
//! repaired wherever it does not. Three of those repairs are *calls*, and they are the
//! ones here: `markers_shift_below_margin`, `markers_evict_oldest` and
//! `markers_rotate_region` are `pub(super)` because the write path in `term.rs` invokes
//! them, mostly on the line beside their selection counterparts. #584 weighed merging the
//! two surfaces into one module on the strength of that pairing and rejected it; the
//! grounds are recorded there.
//!
//! **What a marker's line does *not* go through is this module.** Four sites outside it
//! also move or drop a marker, and a reader who comes here for "everywhere a marker's
//! coordinate changes" will find none of them: primary reflow rewrites `m.line` in place,
//! alt reflow rewrites *and* disposes, alt-leave drains the alt list, and RIS disposes
//! both. All four live in `term.rs` because #584 put reflow and the write path out of
//! scope, which is a boundary of the epic rather than a property of markers.
//!
//! Two declarations stay in `term.rs`, and neither is forced. `Marker` is the element type
//! of the `normal_markers` / `alt_markers` fields, so it sits with the fields it describes.
//! `CommandLine` could have travelled — `mod term` is private, so `pub use
//! term::markers::CommandLine` would keep `justerm_core::CommandLine` byte-identical — but
//! that edits `lib.rs`, which this slice holds untouched, and the ticket does not name it.
//! A child module reads both without any widening.
//!
//! `primary_grid` *did* travel, though the ticket does not name it either: after
//! `command_lines` moved, nothing in `term.rs` called it. It belongs here because command
//! marks anchor **primary** content — on the alt screen their text must be read from the
//! swapped-out grid, not the active one — which is a marker rule, not a general accessor.
//!
//! Visibility follows the callers. Six items are `pub(super)` because the write path and
//! `frame()` invoke them from `term.rs`; that is not a widening, since an item private to
//! `term` was already visible to `term` and all of its descendants. Six are private —
//! every caller travelled with them. The four entry points are public API and keep
//! `pub fn`: an inherent impl's methods are reached through the type, not the module path,
//! so a private child module does not hide them.

use std::collections::VecDeque;

use crate::cell::Cell;
use crate::event::TermEvent;
use crate::grid::Grid;
use crate::serialize::{MarkerId, MarkerKind, MarkerPosition};

use super::{
    CommandLine, CommandRecord, MAX_COMMAND_TEXT, MAX_MARKERS, Marker, MarkerEntry, MarkerIndex,
    Term,
};

impl Term {
    /// The primary-screen grid, wherever it currently lives — swapped into
    /// `alt_grid` while on the alt screen (#192). Command marks anchor *primary*
    /// content, so extracting their text must read this, not the active grid.
    fn primary_grid(&self) -> &Grid {
        if self.on_alt {
            &self.alt_grid
        } else {
            &self.grid
        }
    }

    /// The active buffer's marker list (#177 S0) — alt while on the alt screen,
    /// else normal. Add/rotate/project operate on this; primary-scoped queries
    /// (`command_marks`/`command_lines`) and scrollback eviction read
    /// `normal_markers` directly.
    pub(super) fn markers(&self) -> &VecDeque<Marker> {
        if self.on_alt {
            &self.alt_markers
        } else {
            &self.normal_markers
        }
    }

    /// Mutable [`Self::markers`].
    fn markers_mut(&mut self) -> &mut VecDeque<Marker> {
        if self.on_alt {
            &mut self.alt_markers
        } else {
            &mut self.normal_markers
        }
    }

    /// Register a decoration marker at viewport `row`, returning its stable id
    /// (#118). The row is resolved to an absolute buffer line (like a selection
    /// anchor), so the marker tracks that content through scroll/eviction/reflow.
    pub fn add_marker(&mut self, row: usize) -> MarkerId {
        // On the alt screen this anchors an *alt-scoped* marker (#187): per-buffer
        // storage (#186) keeps it out of the primary list, and it is disposed on
        // alt-leave — xterm's per-buffer `addMarker` + `clearAllMarkers`. No dead
        // sentinel is needed anymore; `markers_mut` routes to the active buffer.
        let line = self.viewport_to_abs(row, 0).line;
        self.push_marker(line, 0, MarkerKind::Plain)
    }

    /// Push a marker anchored at absolute `(line, col)` with `kind`, returning its
    /// id. The shared core of `add_marker` (viewport row, `col = 0`) and OSC-133
    /// command marks (cursor line + column) — one place owns id allocation + the
    /// `markers` list.
    fn push_marker(&mut self, line: usize, col: usize, kind: MarkerKind) -> MarkerId {
        let id = MarkerId(self.next_marker_id);
        self.next_marker_id += 1;
        // #721: this population is allocated by the *stream* — `add_command_mark` appends
        // per OSC 133 sequence, several marks share a line, and eviction only drops one
        // whose line reached abs 0 — so a stream that never emits a newline grows it
        // without bound. Bounded at `MAX_MARKERS`, which the wire's own `u16` group counts
        // derive (the same argument `MAX_COLUMNS` is written from).
        //
        // Overflow retires the **oldest**, not the newest. Refusing the newest is cheaper
        // but permanently kills shell integration for the session: once a pile fills the
        // cap on a line nothing can evict, every later mark would be refused forever. The
        // oldest is also the one already destined to die, and `MarkerDisposed` is the
        // channel scrollback eviction announces that on — so the consumer contract is
        // unchanged rather than extended.
        let mut disposed = Vec::new();
        let markers = self.markers_mut();
        while markers.len() >= MAX_MARKERS {
            // `VecDeque`, not `Vec`, for this line: `remove(0)` would memmove the whole
            // population on *every* push once the cap is reached, turning a memory defect
            // into a throughput one.
            let Some(m) = markers.pop_front() else {
                // Not reachable while `MAX_MARKERS > 0`, and written so that it stays
                // unreachable rather than becoming an infinite loop if it ever is not:
                // an empty deque satisfies `len() >= 0` forever.
                break;
            };
            disposed.push(m.id);
        }
        markers.push_back(Marker {
            id,
            line,
            col,
            kind,
            command: None,
        });
        for id in disposed {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
        // Birth is an occurrence, so it rides the event queue (ADR-0020 R1) — the mirror
        // of the disposal above (#490). A consumer holding a pulled index has no other
        // way to learn of a marker the *stream* created, and without it the index can
        // only ever shrink. Not an epoch bump: that would cost an O(M) re-pull for O(1)
        // information, four times per shell command.
        //
        // The basis rides with the line, exactly as `marker_index` pairs them (#737):
        // `line` is absolute *now*, and the rest of this same `feed` can still evict —
        // which moves every marker, this one included, without touching the epoch. Read
        // against the frame's end-of-batch basis instead, the line is short by whatever
        // the batch evicted after this point.
        //
        // And the epoch rides with it for the same reason one axis up (#741): the basis
        // dates a *uniform* move, and a reflow between this birth and the consumer's drain
        // is not one. `marker_index` answers with all three, so its incremental mirror
        // carries all three — a line whose generation is unstated is one the receiver
        // cannot tell from a current one.
        self.events.push(TermEvent::MarkerCreated {
            id,
            line: line as u32,
            kind,
            evicted_total: self.evicted_total,
            epoch: self.marker_epoch,
        });
        id
    }

    /// Record an OSC 133 command-boundary mark at the cursor's current line
    /// (#158). Ignored on the alt screen: unlike the decoration guards that
    /// per-buffer storage retired (#187), this one stands on a *semantic* — OSC
    /// 133 is shell integration, which only runs on the primary screen, so an alt
    /// 133 is meaningless (there is no command to bound). Command nav/announce read
    /// the *normal* buffer's marks (`command_marks`/`command_lines`, primary-scoped
    /// since #186), so even a stray alt 133 could not reach them — but there is no
    /// value in creating an alt-scoped command mark nothing consumes (#188). The
    /// cursor line is `scrollback ++ screen`-absolute, independent of
    /// `display_offset` (the cursor is always in the grid, never scrollback).
    pub(super) fn add_command_mark(&mut self, kind: MarkerKind) {
        if self.on_alt {
            return;
        }
        let line = self.scrollback.len() + self.cursor.row;
        // `cursor.col` alone is one column short whenever the command exactly filled the row: the
        // cursor that has just written the last cell is held *at* `cols - 1` with `pending_wrap`
        // set, because "one past the last column" is not a column (#562). A command mark's column
        // is an **exclusive** bound on the command text, so it wants precisely that unrepresentable
        // value — `extract_lines` clips `[b_col, c_col)` and `.min(cells.len())` absorbs it.
        //
        // Without this, `$ ` + `abcd` at 6 columns recorded `abc`: no resize involved, so this half
        // of #562 was reachable on a screen that never changed size.
        let col = self.cursor.col + usize::from(self.cursor.pending_wrap);
        self.push_marker(line, col, kind);
        // Both halves of a command that are *not* in the buffer are resolved here, at
        // the mark that reveals them, rather than by a query walking survivors later
        // (#750). See `CommandRecord`.
        match kind {
            MarkerKind::OutputStart => self.capture_command_text(line, col),
            MarkerKind::CommandFinished(exit) => self.attach_exit(exit),
            _ => {}
        }
    }

    /// Freeze the command text on the `OutputStart` mark just pushed (#750).
    ///
    /// `C` is the instant the text is complete and on screen, so this runs the *same*
    /// extraction `command_lines` used to run on demand — `command_start`'s
    /// normalisation and the `[b_col, c_col)` clip — against cells that still hold the
    /// command. Nothing else in this crate can say what the command was: after this
    /// returns, any verb may write those columns.
    ///
    /// A `C` with no open `B` captures nothing, which is the same shape as
    /// `command_lines`'s `pending` taking `None` — a stray `C` bounds no command.
    fn capture_command_text(&mut self, c_line: usize, c_col: usize) {
        let Some((b_line, b_col)) = self.open_command_start() else {
            return;
        };
        let grid = self.primary_grid();
        let (b_line, b_col) = self.command_start(grid, b_line, b_col, c_line);
        let mut text = self.extract_lines(grid, b_line, b_col, c_line, c_col);
        // Bounded for `MAX_MARKERS`' reason: the stream chose the distance between `B`
        // and `C`. Truncated at a `char` boundary so the answer stays valid text.
        if text.chars().count() > MAX_COMMAND_TEXT {
            let end = text
                .char_indices()
                .nth(MAX_COMMAND_TEXT)
                .map_or(text.len(), |(i, _)| i);
            text.truncate(end);
        }
        if let Some(m) = self.normal_markers.back_mut() {
            m.command = Some(Box::new(CommandRecord {
                text: text.into_boxed_str(),
                exit: None,
            }));
        }
    }

    /// The `(line, col)` of the `CommandStart` this `OutputStart` closes, or `None` if
    /// no command is open (#750).
    ///
    /// Walks back to the most recent `B` and stops at the first `C` before it — the
    /// scanning form of `command_lines`'s forward `pending`, and it must stay that way
    /// or the two disagree about which command a `C` bounds. The `OutputStart` just
    /// pushed is skipped.
    fn open_command_start(&self) -> Option<(usize, usize)> {
        self.normal_markers
            .iter()
            .rev()
            .skip(1)
            .find_map(|m| match m.kind {
                MarkerKind::CommandStart => Some(Some((m.line, m.col))),
                MarkerKind::OutputStart => Some(None),
                _ => None,
            })
            .flatten()
    }

    /// Write `D`'s exit code onto the `OutputStart` mark of the command it closes
    /// (#750) — the open one, i.e. the most recent `C` with no `B` after it.
    ///
    /// The `is_none` guard is the one `command_lines` used to carry: a stray second `D`
    /// must not clobber a code that is already recorded.
    fn attach_exit(&mut self, exit: Option<i32>) {
        let open = self
            .normal_markers
            .iter_mut()
            .rev()
            .find_map(|m| match m.kind {
                MarkerKind::OutputStart => Some(Some(m)),
                MarkerKind::CommandStart => Some(None),
                _ => None,
            })
            .flatten();
        if let Some(rec) = open.and_then(|m| m.command.as_mut())
            && rec.exit.is_none()
        {
            rec.exit = exit;
        }
    }

    /// Retire every marker anchored to the **screen row** `row`, announcing each
    /// through `TermEvent::MarkerDisposed` (#750).
    ///
    /// Called where a verb blanks a **whole row in place**, which the three anchor
    /// fixups beside this one cannot see: they repair a marker when the buffer *moves*,
    /// and here the row stays exactly where it is while everything the mark was about
    /// stops existing. Without it `command_lines` answers with commands that are not
    /// there, at document lines that resolve onto blank rows.
    ///
    /// **Takes a screen row and converts it here**, once, because both halves of that
    /// conversion are traps. A marker's `line` is `[scrollback ++ screen]`-absolute
    /// while every erase verb speaks in grid rows; and on the alt screen the same
    /// integers name *primary* lines, so the routing below is what keeps them apart.
    ///
    /// Routes through `markers_mut()`, so an alt-screen erase retires alt markers and
    /// leaves the primary command history alone. Copying `command_marks`' deliberate
    /// `self.normal_markers` here instead would make a `vim` starting up delete the
    /// shell's history.
    ///
    /// **No epoch bump**, by the rule `bump_marker_epoch` states: the epoch dates a
    /// *move* no offset repairs, and this moves nothing. A consumer hears disposal on
    /// its own channel and drops the entry — the same shape as `markers_evict_oldest`.
    pub(super) fn dispose_markers_on_row(&mut self, row: usize) {
        // `display_offset` is deliberately absent: a write always lands in the grid,
        // whatever the viewport is scrolled to (the same expression `add_command_mark`
        // uses for the cursor).
        let line = self.scrollback.len() + row;
        let mut disposed = Vec::new();
        self.markers_mut().retain(|m| {
            if m.line == line {
                disposed.push(m.id);
                false
            } else {
                true
            }
        });
        for id in disposed {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
    }

    /// The OSC 133 command-boundary marks in buffer order — `(id, absolute line,
    /// kind)` (#158). Plain decoration markers (#118) are excluded. The consumer
    /// pairs prompt/command/finished marks and drives navigation/announce policy
    /// (#160); core only parses and anchors them.
    ///
    /// **Instantaneous, deliberately (#742).** The lines are undated and move on both
    /// of `marker_index`'s axes, so the contract is *re-ask*, not *rebase* — see
    /// `Engine::command_marks` for the derivation, which is the docs.rs surface a
    /// consumer actually reads. Two properties keep that honest and are facts about
    /// *this* function rather than preferences: the scope below is constant, so a
    /// re-ask always answers and an empty answer can only mean disposal; and the lines
    /// are primary even on alt, so they are not in the active buffer's space.
    ///
    /// Changing either — routing this through `markers()`, or filtering by anything
    /// buffer-dependent — invalidates the contract above, not just this line.
    pub fn command_marks(&self) -> Vec<(MarkerId, usize, MarkerKind)> {
        // Primary-scoped: OSC-133 shell integration marks live on the normal
        // buffer, so command nav/announce read it even while on the alt screen.
        self.normal_markers
            .iter()
            .filter(|m| m.kind != MarkerKind::Plain)
            .map(|m| (m.id, m.line, m.kind))
            .collect()
    }

    /// The executed shell commands recovered from OSC-133 marks, in buffer order
    /// (#166) — the data behind screen-reader command navigation. Each
    /// [`CommandLine`] pairs a CommandStart(B) with the following OutputStart(C)
    /// to extract the *typed command* (the prompt before B and the output after C
    /// excluded via the captured columns, VSCode `extractCommandLine` parity), and
    /// attaches the trailing CommandFinished(D) exit. A command still being typed
    /// (B with no C yet) is not navigable — its text has no bound — so it is
    /// omitted until output starts.
    ///
    /// The answer is instantaneous, and its lines are document lines into
    /// [`Term::accessible_text`] *on the primary screen* — the contract a caller reads
    /// is on `Engine::command_lines`, pinned by `tests/command_lines_document.rs`
    /// (#743, ADR-0029 D6). Note the omission above is why absence here means *gone or
    /// not yet complete* rather than the flat "disposed" that holds for
    /// [`Self::command_marks`]; both are absences a re-ask resolves, which is what
    /// D3.2 needs of them.
    pub fn command_lines(&self) -> Vec<CommandLine> {
        let mut out: Vec<CommandLine> = Vec::new();
        // (B line, B col) awaiting its matching C. Marks arrive in buffer order.
        let mut pending: Option<(usize, usize)> = None;
        // Primary-scoped (see `command_marks`): the normal buffer's marks.
        for m in &self.normal_markers {
            match m.kind {
                MarkerKind::CommandStart => pending = Some((m.line, m.col)),
                MarkerKind::OutputStart => {
                    if let Some((b_line, b_col)) = pending.take() {
                        // The text and the exit are read off the mark, frozen when this
                        // `C` and its `D` arrived (#750) — see `CommandRecord`. A `C`
                        // with no record bounds no command (a stray one, or a mark that
                        // predates nothing this crate can produce), so it is skipped
                        // rather than reported empty.
                        let Some(rec) = m.command.as_deref() else {
                            continue;
                        };
                        // The *line* is still derived, and deliberately: it is the one
                        // half a fixup does maintain, so freezing it would break the
                        // thing that already works. Where the typed command begins is
                        // not always where B was emitted — a prompt that ends its row
                        // leaves B past that row's content, and the command really
                        // starts on the next line. Command marks anchor primary
                        // content, so this reads the primary grid even on alt (#192).
                        let (b_line, _) =
                            self.command_start(self.primary_grid(), b_line, b_col, m.line);
                        out.push(CommandLine {
                            line: self.doc_line_of(self.primary_grid(), b_line),
                            command: rec.text.to_string(),
                            exit: rec.exit,
                        });
                    }
                }
                MarkerKind::CommandFinished(_) => {
                    // Nothing: `D`'s code was written onto its `OutputStart` mark when
                    // it arrived (#750). Pairing here meant pairing over *survivors*,
                    // and a disposal that broke the run re-parented the next code onto
                    // the previous command — measured, `a0` inheriting `a1`'s `Some(2)`.
                }
                MarkerKind::Plain | MarkerKind::PromptStart => {}
            }
        }
        out
    }

    /// Advance an OSC-133 `CommandStart` position past any hard-ended line that holds no command
    /// text at or after it, stopping before `end` (the matching `OutputStart`).
    ///
    /// B is emitted at the cursor, so a prompt that fills — or merely ends — its row leaves the mark
    /// in that row's trailing blanks (#562). Two things then go wrong if the raw position is used:
    /// `extract_lines` selects an empty run and, because the row is hard-ended, flushes it with a
    /// `\n` the command never contained; and `doc_line_of` names the prompt's line rather than the
    /// command's. Both were reachable **without any resize** — the row only has to end before its
    /// width, which an 8-column row holding a 6-column prompt does.
    ///
    /// Only hard-ended rows advance. On a soft-wrapped row the continuation is the same logical
    /// line, its trailing blanks are real content (a space at a wrap boundary was typed), and no
    /// `\n` is flushed there anyway.
    fn command_start(&self, grid: &Grid, line: usize, col: usize, end: usize) -> (usize, usize) {
        let (mut line, mut col) = (line, col);
        while line < end && !self.row_in(grid, line).is_wrapped() {
            let cells = self.line_in(grid, line);
            if col < cells.len() && !cells[col..].iter().all(Cell::is_blank) {
                break;
            }
            line += 1;
            col = 0;
        }
        (line, col)
    }

    /// The document (logical) line index that absolute buffer line `abs` renders
    /// into within [`Term::accessible_text`] — the number of hard line-ends before
    /// it (soft-wrapped rows share one logical line). Primary-screen coordinates,
    /// matching `accessible_text`'s `start = 0` for the primary screen; command
    /// marks are primary-only. O(abs) per call — fine for an on-demand query over
    /// the handful of commands in a session.
    fn doc_line_of(&self, grid: &Grid, abs: usize) -> usize {
        (0..abs)
            .filter(|&l| !self.row_in(grid, l).is_wrapped())
            .count()
    }

    /// Remove a marker by id (#118). Disposing it fires `MarkerDisposed` so the
    /// consumer's cleanup is one path whether the marker left by eviction or by
    /// this explicit call (xterm's `dispose()` likewise always fires onDispose).
    /// A no-op for an unknown/already-disposed id.
    pub fn remove_marker(&mut self, id: MarkerId) {
        // Id-based, buffer-agnostic: search both lists (ids are unique across
        // buffers) so a marker is removed whichever screen it lives on (#177 S0).
        let before = self.normal_markers.len() + self.alt_markers.len();
        self.normal_markers.retain(|m| m.id != id);
        self.alt_markers.retain(|m| m.id != id);
        if self.normal_markers.len() + self.alt_markers.len() != before {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
    }

    /// Every live marker of the active buffer, with the basis that keeps the answer
    /// usable (#490). The pull half of the marker surface: a consumer asks once and
    /// rebases per frame rather than being handed every marker in every frame.
    ///
    /// Ordering is the engine's own, which is the precedence a consumer joins
    /// decorations by (#458/#461) — the same reason `marker_positions` does not sort.
    pub fn marker_index(&self) -> MarkerIndex {
        MarkerIndex {
            markers: self
                .markers()
                .iter()
                .map(|m| MarkerEntry {
                    id: m.id,
                    line: m.line as u32,
                    kind: m.kind,
                })
                .collect(),
            evicted_total: self.evicted_total,
            epoch: self.marker_epoch,
        }
    }

    /// Declare that a held marker line has gone stale for a reason the
    /// `evicted_total` delta cannot express (#490).
    ///
    /// Every caller is a site that moves marker lines **non-uniformly** — a region
    /// rotate touches only the markers inside the region, a reflow rewrites them
    /// outright, an alt switch changes which buffer the answer even describes. The
    /// bump is deliberately *not* placed on disposal: a consumer hears that on
    /// `MarkerDisposed` and drops the entry without asking for the rest again.
    pub(super) fn bump_marker_epoch(&mut self) {
        self.marker_epoch = self.marker_epoch.wrapping_add(1);
    }

    /// The marker analogue of `selection_shift_below_margin` (#449) — primary
    /// only, because the accrual branch that needs it is primary-only.
    pub(super) fn markers_shift_below_margin(&mut self, from: usize) {
        let mut moved = false;
        for m in &mut self.normal_markers {
            if m.line >= from {
                m.line += 1;
                moved = true;
            }
        }
        if moved {
            self.bump_marker_epoch();
        }
    }

    /// Shift markers down one absolute line after the oldest history line is
    /// evicted; a marker *on* that line (abs 0) has left the buffer, so it is
    /// disposed and announced (#118) — the marker analogue of
    /// `selection_evict_oldest`, but a list with per-marker disposal.
    pub(super) fn markers_evict_oldest(&mut self) {
        // Scrollback eviction is primary-only (the alt screen has none).
        let mut disposed = Vec::new();
        self.normal_markers.retain_mut(|m| {
            if m.line == 0 {
                disposed.push(m.id);
                false
            } else {
                m.line -= 1;
                true
            }
        });
        for id in disposed {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
    }

    /// Rotate markers within an in-screen region scroll of absolute lines
    /// `[top, bottom]` (`up` = a line dropped at `top`, else at `bottom`) — the
    /// marker analogue of `selection_rotate_region`. A marker on the dropped edge
    /// has left the buffer, so it is disposed and announced (#118).
    pub(super) fn markers_rotate_region(&mut self, top: usize, bottom: usize, up: bool) {
        let mut disposed = Vec::new();
        let mut moved = false;
        self.markers_mut().retain_mut(|m| {
            if m.line < top || m.line > bottom {
                return true; // outside the region — unchanged
            }
            let dropped_edge = if up { top } else { bottom };
            if m.line == dropped_edge {
                disposed.push(m.id);
                false
            } else {
                m.line = if up { m.line - 1 } else { m.line + 1 };
                moved = true;
                true
            }
        });
        for id in disposed {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
        // Only a *surviving* marker that moved invalidates a held index (#490). A
        // rotate that merely disposed the edge marker, or found none inside the
        // region at all, leaves every held line correct — and gating on that is what
        // keeps a TUI scrolling a region from forcing a re-pull per line.
        if moved {
            self.bump_marker_epoch();
        }
    }

    /// The active buffer's markers projected onto the current viewport — one
    /// `MarkerPosition` per marker whose line is visible, off-screen markers
    /// omitted. The alt screen projects its own (alt-scoped) markers now (#187);
    /// they are disposed on alt-leave, so a primary frame never shows them.
    pub(super) fn marker_positions(&self) -> Vec<MarkerPosition> {
        let top = self.scrollback.len() - self.display_offset;
        let rows = self.grid.rows();
        self.markers()
            .iter()
            .filter_map(|m| {
                let row = m.line.checked_sub(top)?;
                (row < rows).then_some(MarkerPosition {
                    id: m.id,
                    row,
                    kind: m.kind,
                })
            })
            .collect()
    }
}
