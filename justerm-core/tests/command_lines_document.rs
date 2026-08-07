//! #743 — `CommandLine::line` is a **document** line, and that is two facts, not one.
//! ADR-0029 governs the first and explicitly puts the second out of its scope, so this
//! file pins both: prose alone is what failed in #737, and D6 is why a re-ask discharge
//! owes a test rather than a sentence.
//!
//! **When it is true (ADR-0029 D3).** `Engine::command_lines` takes the *re-ask*
//! discharge, like `Engine::command_marks` and unlike `Engine::marker_index` — its clock
//! is a user action and its frame of reference never flips. That is the positive ground,
//! and it is the whole of it.
//!
//! **It is not that carrying is impossible — that was measured and it is false.** The
//! eviction site pops one row and knows whether it was a line-end, so a counter beside
//! `evicted_total` would date this space under eviction exactly. What makes carry the
//! *expensive* discharge is that the document space also moves on an axis the absolute
//! space does not have at all: flipping a row's wrap bit, which ordinary output does,
//! with nothing evicted and no epoch bump. Dating it would therefore take a line-end
//! counter **and** a generation of its own — ADR-0029 D5's apparatus, duplicated for a
//! second coordinate space. That is alternative (D), deferred on cost.
//!
//! Two tests carry that: the first isolates the eviction divergence (no published
//! scalar distinguishes the eviction that moves this space from the one that does not),
//! and the last isolates the wrap-bit axis (no published scalar sees it at all).
//!
//! **Which document it indexes.** A document line only means anything against a
//! document, and the one it names is `[scrollback ++ primary]` — which is what
//! `Engine::accessible_text` returns *only while the primary screen is active*. On the
//! alt screen the two queries answer about different buffers at the same instant. That
//! is not an instant problem and no basis, epoch or validity window addresses it, which
//! is why ADR-0029's scope paragraph leaves it out and this file carries it.

use justerm_core::Engine;

/// The command's document line, as a caller of `command_lines` receives it.
fn doc(e: &Engine) -> Vec<usize> {
    e.command_lines().iter().map(|c| c.line).collect()
}

/// The same commands' **absolute** lines, from the sibling query. The control: it is
/// the space `evicted_total` dates, and the space a naive rebase assumes.
fn abs(e: &Engine) -> Vec<usize> {
    e.command_marks().into_iter().map(|(_, l, _)| l).collect()
}

/// The divergence, isolated to the single eviction that causes it.
///
/// `doc_line_of` counts rows that do **not** soft-wrap into the next one, so a
/// continuation row contributes nothing to a document index. Evicting one therefore
/// moves every absolute line by 1 and every document line by **0** — and the caller
/// receives no value that distinguishes this eviction from the ones either side of it,
/// which do move both spaces together.
#[test]
fn evicting_a_continuation_row_moves_the_absolute_space_and_not_the_document() {
    // 8 columns, so a 12-character line occupies two rows as one logical line.
    let mut e = Engine::with_scrollback(8, 4, 12);
    e.feed(b"AAAAAAAAAAAA\r\n");
    for _ in 0..10 {
        e.feed(b"x\r\n");
    }
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07o\r\n\x1b]133;D;0\x07");

    // The window this test asserts inside has to exist first (#639): the collapse is
    // what makes the two spaces differ at all, and without it every assertion below
    // would pass vacuously on a buffer where doc == abs by construction.
    let rows_total = e.scrollback_len() + 4;
    assert!(
        e.accessible_text().lines().count() < rows_total,
        "the fixture must actually contain a soft-wrapped line, or the divergence \
         it is built to catch cannot occur"
    );

    assert_eq!(abs(&e), vec![12, 12, 12, 13], "absolute, as first answered");
    assert_eq!(doc(&e), vec![11], "document, as first answered");

    // Feed until the first row is evicted. That row is the *first half* of the
    // wrapped pair — the one carrying the soft-wrap link — so it is not a line-end.
    while e.marker_index().evicted_total == 0 {
        e.feed(b"y\r\n");
    }

    assert_eq!(
        e.marker_index().evicted_total,
        1,
        "exactly one row left the buffer"
    );
    assert_eq!(
        abs(&e),
        vec![11, 11, 11, 12],
        "the absolute space moved by that one row, which is what `evicted_total` dates"
    );
    assert_eq!(
        doc(&e),
        vec![11],
        "and the document space did not move at all: the evicted row was a \
         continuation, so it was never a document line. A caller rebasing by the \
         `evicted_total` delta computes 10 here and reveals the wrong command."
    );

    // The control, in the same run: the very next eviction pops a hard-ended row and
    // the two spaces move together again. Both behaviours from one fixture is the
    // point — the wrong rule is right most of the time, which is why it survives.
    e.feed(b"y\r\n");
    assert_eq!(e.marker_index().evicted_total, 2);
    assert_eq!(abs(&e), vec![10, 10, 10, 11], "absolute, again by one");
    assert_eq!(
        doc(&e),
        vec![10],
        "document, this time by one as well — the case that makes the naive rebase \
         look correct"
    );
}

/// What the coordinate *means*, pinned positively: it indexes the lines of
/// `accessible_text`. Without this the tests above only say the number changes, not
/// that anything reads it — and the whole defect is a mismatch between a number and a
/// document nobody asserted it against.
///
/// The fixture puts a soft-wrapped line **above** the command on purpose. Without one
/// the document index and the absolute index are the same integer, and this test then
/// passes just as happily against an engine that conflates the two spaces — measured,
/// not assumed: it did, until the wrapped line was added.
#[test]
fn the_line_indexes_accessible_text_while_the_primary_screen_is_active() {
    // 8 columns: the first line occupies two rows and one document line.
    let mut e = Engine::with_scrollback(8, 5, 20);
    e.feed(b"AAAAAAAAAAAA\r\n");
    e.feed(b"b\r\n");
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07o\r\n\x1b]133;D;0\x07");

    let cmd = &e.command_lines()[0];
    let text = e.accessible_text();
    let lines: Vec<&str> = text.lines().collect();

    assert_eq!(lines, vec!["AAAAAAAAAAAA", "b", "$ lso"]);
    assert_eq!(
        e.command_marks()[0].1,
        3,
        "the command sits on absolute row 3 — the wrapped pair took rows 0 and 1"
    );
    assert_eq!(
        cmd.line, 2,
        "but on document line 2, because the pair collapsed into one. The two spaces \
         are different integers here, which is what makes the next assertion mean \
         something"
    );
    assert_eq!(
        lines[cmd.line], "$ lso",
        "the document line is an index into this document, not into the buffer"
    );
}

/// The axis ADR-0029 puts out of its own scope: *which* document. Both answers are
/// individually correct and deliberately so — `accessible_text` floors at
/// `abs_floor()` because the AT must read what is on screen, and `command_lines`
/// reads `primary_grid()` because OSC-133 marks are primary-only (#192). Nothing
/// stated that they therefore stop being the same document, and a consumer holding
/// both has no value that tells it so.
///
/// The fixture must have **real scrollback** and an alt document **long enough that
/// the held line still resolves**, or it pins the mild half of the defect and reads
/// as covering the whole of it. Both conditions were measured, not reasoned. With a
/// grid tall enough to hold everything, `scrollback.len()` is 0 and `abs_floor()`
/// returns 0 on both screens, so the alt document is short merely because the alt grid
/// is blank and the test passes with the floor deleted. And with a *short* alt
/// document the index falls off the end, which is the benign outcome: a consumer's
/// reveal fails and nothing happens. The reachable case is the opposite one — a TUI
/// fills its screen, which is what an alt excursion normally looks like.
#[test]
fn on_the_alt_screen_the_same_line_names_entirely_different_content() {
    // The command lands early (document line 1) and 20 lines of output push real
    // scrollback behind it, so the held index is comfortably inside a 5-line alt
    // document while `abs_floor()` still has work to do.
    let mut e = Engine::with_scrollback(16, 6, 40);
    e.feed(b"a\r\n");
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");
    for i in 0..20 {
        e.feed(format!("out {i}\r\n").as_bytes());
    }
    let on_primary = doc(&e);
    assert_eq!(on_primary, vec![1]);
    assert_eq!(
        e.scrollback_len(),
        17,
        "the floor below has something to do"
    );
    assert_eq!(
        e.accessible_text().lines().nth(1),
        Some("$ lsout"),
        "on the primary screen the held line names the command"
    );

    e.feed(b"\x1b[?1049h");
    for i in 0..5 {
        e.feed(format!("TUI row {i}\r\n").as_bytes());
    }

    assert_eq!(
        doc(&e),
        on_primary,
        "unchanged: this query is primary-scoped and does not follow the active \
         buffer, exactly like `command_marks` (#742) — which is what keeps a re-ask \
         answering"
    );
    let text = e.accessible_text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        5,
        "the document that line now indexes is the alt screen's, with the primary \
         history correctly floored out of it because the AT must read what is on screen"
    );
    assert_eq!(
        lines[on_primary[0]], "TUI row 1",
        "and the SAME index resolves — in range, to unrelated content. This is the \
         reachable shape and the dangerous one: a consumer reveals successfully, moves \
         the reading cursor onto a TUI row, and announces the command's text beside it. \
         No bounds check can catch this, which is why the contract has to be re-ask \
         rather than a guard"
    );
}

/// The milder half of the same defect, kept because it is the half a bounds check
/// *can* catch, and a consumer is entitled to know which of the two it is handling:
/// when the alt document is shorter than the held index, the index simply names
/// nothing.
#[test]
fn on_a_short_alt_screen_the_line_falls_off_the_end_instead() {
    // 2 rows, so the four primary lines really do reach scrollback (measured: 3).
    let mut e = Engine::with_scrollback(16, 2, 8);
    e.feed(b"a\r\nb\r\nc\r\n");
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");
    let on_primary = doc(&e);
    assert_eq!(on_primary, vec![3]);
    assert_eq!(e.scrollback_len(), 3, "the floor below has something to do");

    e.feed(b"\x1b[?1049h");
    e.feed(b"TUI");

    assert_eq!(doc(&e), on_primary, "still primary-scoped");
    assert_eq!(e.accessible_text(), "\nTUI");
    assert!(
        e.accessible_text().lines().count() <= on_primary[0],
        "line 3 of a two-line document names nothing at all"
    );
}

/// The property the re-ask discharge rests on (ADR-0029 D3.2): the frame of reference
/// never flips, so a re-ask always answers.
///
/// **What absence means here is *not* what it means for `command_marks`, and copying
/// the sibling's sentence was wrong.** That query reports every mark, so an empty
/// answer can only mean disposal. This one omits a command whose output has not
/// started (`markers.rs`: a B with no C yet has no bound on its text), so absence
/// means *gone* **or** *not yet complete* — both of which a re-ask resolves by itself.
/// The distinction that matters for D3.2 is a different one: absence never means
/// *"you are on the other screen"*, because that is the one a re-ask can never
/// recover from.
///
/// It is also why the tempting fix for the tests above is unavailable. Returning an
/// empty list on the alt screen would introduce exactly that meaning, failing D3.2 and
/// forcing the carry discharge — which is the expensive one here (see the module doc).
#[test]
fn a_re_ask_answers_on_either_screen_and_absence_never_means_the_other_screen() {
    // Two rows of content first, so the alt-screen assertion below is not standing on
    // an empty scrollback where `abs_floor()` is a no-op either way.
    let mut e = Engine::with_scrollback(8, 2, 8);
    e.feed(b"p\r\nq\r\n");

    // A command typed but not yet running: absent, with nothing disposed.
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls");
    assert!(
        e.command_lines().is_empty(),
        "absence #1 — the command has no output yet, so its text has no bound"
    );
    assert_eq!(
        e.command_marks().len(),
        2,
        "and the marks are alive, so this absence is not disposal"
    );

    e.feed(b"\x1b]133;C\x07o\r\n\x1b]133;D;0\x07");
    assert_eq!(e.command_lines().len(), 1, "output started: now navigable");
    assert!(e.scrollback_len() > 0, "the floor has something to do");

    e.feed(b"\x1b[?1049h");
    assert_eq!(
        e.command_lines().len(),
        1,
        "re-asking on the alt screen still answers — absence never means \
         'you are on the other screen', which is the meaning no re-ask could undo"
    );

    e.feed(b"\x1b[?1049l");
    for _ in 0..12 {
        e.feed(b"z\r\n");
    }
    assert!(
        e.command_lines().is_empty(),
        "absence #2 — evicted off the top. Both absences are ones a re-ask resolves \
         on its own, which is what D3.2 needs of them"
    );
}

/// The axis the eviction test does not reach, and the one that makes the carry
/// discharge expensive rather than merely unnecessary.
///
/// A document line moves when a row's **wrap bit** flips, and ordinary output flips
/// one: rewriting a short row with enough text to overflow it joins two document lines
/// into one. Measured here — nothing else moves at all. `evicted_total` is still,
/// `marker_epoch` is still, and the *absolute* lines are still, because no row was
/// added, removed or shifted. So this is not a second instance of the eviction axis;
/// it is a motion the absolute space does not have, on a coordinate that leaves core.
///
/// This is why the sibling's shape would not have been enough even if it had been
/// copied: dating this space would take a counter for line-ends **and** a generation of
/// its own, duplicating ADR-0029 D5's whole apparatus for a second coordinate space.
/// The record's alternative (D) is deferred on that cost.
#[test]
fn flipping_a_wrap_bit_moves_the_document_line_and_nothing_else() {
    // 8 columns: rows 0 and 1 start hard-ended, so they are two document lines.
    let mut e = Engine::with_scrollback(8, 6, 40);
    e.feed(b"ab\r\ncd\r\n");
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07o\r\n\x1b]133;D;0\x07");

    let before = e.marker_index();
    assert_eq!(doc(&e), vec![2], "two document lines above the command");
    assert_eq!(abs(&e), vec![2, 2, 2, 3]);

    // Rewrite row 0 with 12 characters: it now soft-wraps into row 1, and the two
    // rows collapse into one document line. No scroll, no eviction, no resize.
    e.feed(b"\x1b[1;1HAAAAAAAAAAAA");

    let after = e.marker_index();
    assert_eq!(
        doc(&e),
        vec![1],
        "the document line moved by one, from the byte stream alone"
    );
    assert_eq!(
        abs(&e),
        vec![2, 2, 2, 3],
        "while the absolute lines did not move — this axis is the document space's own"
    );
    assert_eq!(
        after.evicted_total, before.evicted_total,
        "and `evicted_total` cannot see it: nothing was evicted"
    );
    assert_eq!(
        after.epoch, before.epoch,
        "nor can the epoch: no reflow, rotate, alt switch or RIS happened. A holder \
         watching both of ADR-0029 D5's scalars is still wrong here"
    );
}
