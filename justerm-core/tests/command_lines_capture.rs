//! #750, Step 4 — the real round-trip. A synthetic fixture proves the rule the fixture
//! was written from; this proves the rule against bytes a real shell actually emitted.
//!
//! `fixtures/osc133_clear.raw` is a `script(1)` recording made on the RHEL 9.2 VM
//! (`TERM=xterm-256color`, `LC_ALL=C.UTF-8`, 10×40, `stty size` confirmed inside the
//! same invocation): a real `bash 5.1` under a real pty with OSC-133 integration in
//! `PS1`/`PROMPT_COMMAND`/`DEBUG`, driven by `expect`. It contains
//! `echo one` typed **with a correction** (four backspaces), `echo two`, a real
//! `clear`, and `echo three` afterwards.
//!
//! Three things it measured that no synthetic fixture would have:
//!
//! 1. **A real `clear` emits `ESC[H ESC[2J ESC[3J`** — so `ED 2`, the arm this fix is
//!    mostly about, is genuinely the dominant path. (`ED 3` is unimplemented here; see
//!    the second test.)
//! 2. **`readline` emits `ESC[K` on the row that carries `B`, while the command is
//!    being typed** — four of them, one per backspace, all between `B` and `C`. That is
//!    the reference-free half of why `EL` retires nothing: an `EL` rule has to answer
//!    for a mark that is mid-command, and this is what it would be answering for.
//!    (Measured as a *partial* erase at the cursor — **not** the `\r ESC[K` full-row
//!    redraw this fix's first draft of doc-comments asserted without having looked. The
//!    correction matters: at this size readline never blanks a whole row, so on these
//!    bytes an `EL` rule scoped to whole rows would have been harmless. Excluding `EL`
//!    therefore rests on the two converging references, with this as evidence that `EL`
//!    lands between `B` and `C` at all — not as proof that a whole-row `EL` rule would
//!    break shell integration.)
//! 3. **The `DEBUG` trap fires `C` more than once per command**, including before the
//!    first prompt — so the pairing walk meets stray `OutputStart` marks in real
//!    traffic, not only in an adversarial test.

use justerm_core::Engine;

fn replay() -> Engine {
    let raw = include_bytes!("fixtures/osc133_clear.raw");
    let mut e = Engine::with_scrollback(40, 10, 200);
    e.feed(raw);
    e
}

fn commands(e: &Engine) -> Vec<String> {
    e.command_lines().into_iter().map(|c| c.command).collect()
}

/// The whole point, on real bytes: after the `clear` in the middle of the recording,
/// the three commands that ran before it are gone from the answer and the one that ran
/// after it appears once.
///
/// Measured pre-fix on these same bytes, and it is worth quoting because no synthetic
/// fixture produced anything this bad:
///
/// ```text
/// ["", "COMMAND_EXIT_CODE=\"0\"]\n", "", "echo three\n", "exit\n"]
/// ```
///
/// Five entries for two surviving commands, two of them empty, and one re-reading the
/// shell integration's own `PROMPT_COMMAND` text through a dead mark's columns — a
/// string a screen reader would have announced as a command the user ran.
///
/// **The assertion is on the exact list, and that is deliberate.** The first draft
/// asserted *"no surviving entry contains `one` or `two`"*, which is **vacuously true
/// pre-fix** — the stale entries were empty or garbage, so they contained neither. It
/// passed in both states and read as coverage. Turning the fix off is what exposed it,
/// which is the whole reason that step exists.
///
/// **Which half this capture can observe, measured rather than assumed.** With the
/// disposal turned off it goes red; with the *capture* turned off it stays green, and
/// that is a property of the material: this recording never rewrites a command's row
/// after that command's `C`, so re-extracting from live cells still yields the right
/// text. A third test asserting the edited command (`echoo hi` → four backspaces →
/// `echo one`) was written, found green in **both** states, and deleted — the edit
/// completes before `C`, so the cells hold the submitted form either way and the
/// assertion could not fail. The capture half is pinned by `command_lines_content.rs`
/// instead, against overwrite / `ICH` / `DCH` / a wrapped command. A recording that
/// would discriminate it needs a program repainting the primary screen over a finished
/// command, which this one does not contain.
#[test]
fn a_real_clear_leaves_only_the_commands_that_ran_after_it() {
    let e = replay();

    assert_eq!(
        commands(&e),
        vec!["echo three\n".to_string(), "exit\n".to_string()],
        "exactly the two commands that ran after the clear — no empty entry, no \
         re-borrowed text, no duplicate"
    );
}

/// `ED 3` (erase scrollback) is in the recording and is a no-op here, so a mark that
/// scrolled off survives a `clear` that a real terminal would have erased it with.
///
/// Recorded rather than filed: `ED 3` is unimplemented, and `term.rs`'s arm for it
/// already carries the anchor-fixup obligation whoever implements it inherits. This
/// test exists so that the obligation has a *measured* consequence attached to it
/// instead of only a comment.
///
/// It is a **characterisation** test, not a proof of this fix — it is green with either
/// half turned off, by construction, since it asserts that something does *not* happen.
/// What it can fail on is the change it is aimed at: implementing `ED 3` without
/// retiring the marks it erases will not move it, and implementing it *with* the fixup
/// will, which is the moment to rewrite it.
#[test]
fn ed_3_is_still_a_no_op_so_scrollback_marks_outlive_a_real_clear() {
    // 3 rows, so the early commands are pushed into scrollback before the clear.
    let raw = include_bytes!("fixtures/osc133_clear.raw");
    let mut e = Engine::with_scrollback(40, 3, 200);
    e.feed(raw);

    let cmds = commands(&e);
    assert!(
        cmds.iter().any(|c| c.contains("one") || c.contains("two")),
        "a pre-clear command that reached scrollback is still reported, because \
         ED 3 does not erase it: {cmds:?}"
    );
}
