//! A miniature *consumer* that answers a terminal's queries with **this engine's** replies,
//! so a capture harness can close the loop without owning any part of the VT layer (#891).
//!
//! Reads length-prefixed frames of VT bytes on stdin and writes a length-prefixed frame of
//! reply bytes back for each one, so the caller never guesses when a reply is finished.
//! `justerm-core` does no I/O (`CLAUDE.md`), which is why the pty lives in the harness that
//! drives this and not in here.
//!
//! **It is a consumer and not a pipe, and that is the whole reason it exists.** `drain_replies`
//! alone answers DA1, DA2, DSR, DECRQM and the kitty flags query; the four colour and clipboard
//! query families reach a consumer as a `TermEvent` and are answered by *policy* (ADR-0017), so
//! a harness that only forwarded `drain_replies` would leave them silent. The policy is on the
//! command line rather than compiled in, so the capture script records which one a fixture was
//! recorded under.
//!
//! Usage: `reply_filter <cols> <rows> <fg-spec> <bg-spec>`

use std::io::{Read, Write};

use justerm_core::{Engine, TermEvent};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        eprintln!("usage: reply_filter <cols> <rows> <fg-spec> <bg-spec>");
        std::process::exit(2);
    }
    let cols: usize = args[0].parse().expect("cols");
    let rows: usize = args[1].parse().expect("rows");
    let fg = args[2].clone();
    let bg = args[3].clone();

    // Validated here rather than tolerated at query time. A spec the policy cannot read would
    // still be handed to the application verbatim over OSC 11, and then *classified* by the
    // fallback below — so `#ffffff` would give vim a white background and tell it the scheme is
    // dark, silently, inside a checked-in fixture. Refuse to record rather than record that.
    for (what, spec) in [("foreground", &fg), ("background", &bg)] {
        if mean_channel(spec).is_none() {
            eprintln!("reply_filter: {what} spec {spec:?} is not `rgb:RRRR/GGGG/BBBB`");
            std::process::exit(2);
        }
    }

    let mut engine = Engine::new(cols, rows);
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();

    loop {
        let mut len = [0u8; 4];
        if stdin.read_exact(&mut len).is_err() {
            break;
        }
        let mut chunk = vec![0u8; u32::from_le_bytes(len) as usize];
        if stdin.read_exact(&mut chunk).is_err() {
            break;
        }

        engine.feed(&chunk);
        answer_events(&mut engine, &fg, &bg);
        let reply = engine.drain_replies();

        stdout
            .write_all(&(reply.len() as u32).to_le_bytes())
            .expect("write len");
        stdout.write_all(&reply).expect("write reply");
        stdout.flush().expect("flush");
    }
}

/// The policy half. Every arm here is a choice a real consumer would also have to make, and
/// none of them is the engine's to make.
fn answer_events(engine: &mut Engine, fg: &str, bg: &str) {
    for ev in engine.drain_events() {
        match ev {
            TermEvent::QueryForeground { terminator } => engine.report_foreground(fg, terminator),
            TermEvent::QueryBackground { terminator } => engine.report_background(bg, terminator),
            TermEvent::QueryCursorColor { terminator } => {
                engine.report_cursor_color(fg, terminator)
            }
            TermEvent::QueryPaletteColor { index, terminator } => {
                // One flat spec for every index: this harness has no palette, and a consumer
                // that had one would answer from it. What matters for a capture is that the
                // query is *answered*, not which colour comes back.
                engine.report_palette_color(index, fg, terminator);
            }
            // Dark or light is derived from the background actually handed back, so the two
            // cannot disagree inside one recording.
            TermEvent::ColorSchemeQuery => engine.report_color_scheme(is_dark(bg)),
            // A clipboard read is refused, and a refusal is silence (#841). Answering would put
            // this machine's clipboard into a checked-in fixture.
            TermEvent::QueryClipboard { .. } => {}

            // Everything a consumer acts on rather than answers. Spelled out so the arm below
            // means one thing only.
            TermEvent::Title(_)
            | TermEvent::Bell
            | TermEvent::Cwd(_)
            | TermEvent::ColumnMode { .. }
            | TermEvent::SetPaletteColor { .. }
            | TermEvent::SetForeground(_)
            | TermEvent::SetBackground(_)
            | TermEvent::ResetPaletteColor(_)
            | TermEvent::SetCursorColor(_)
            | TermEvent::ResetCursorColor
            | TermEvent::ResetForeground
            | TermEvent::ResetBackground
            | TermEvent::ClipboardStore { .. }
            | TermEvent::MarkerDisposed { .. }
            | TermEvent::MarkerCreated { .. } => {}

            // `TermEvent` is `#[non_exhaustive]`, so this arm is required and an exhaustive
            // match cannot be written from outside the crate — the compiler will not announce a
            // new variant here. With the other twenty named above, reaching this means a variant
            // was added after this file was written, and if it is a *query* the capture will
            // record justerm as silent about something it in fact answers. Loud, because the
            // failure this whole fixture exists to prevent is an absence read as data.
            other => {
                eprintln!("reply_filter: unhandled TermEvent, capture may be wrong: {other:?}")
            }
        }
    }
}

/// The mean channel of `rgb:RRRR/GGGG/BBBB` as a fraction of full scale, or `None` if any part
/// of it does not parse. Each channel is scaled by its own digit count, so `rgb:00/00/00` and
/// `rgb:0000/0000/0000` agree. **Every** channel has to read — a partial parse would average
/// the ones it happened to understand and look like an answer.
fn mean_channel(spec: &str) -> Option<f64> {
    let body = spec.strip_prefix("rgb:")?;
    let mut sum = 0.0f64;
    let mut n = 0.0f64;
    for part in body.split('/') {
        let v = u32::from_str_radix(part, 16).ok()?;
        let full = 16f64.powi(i32::try_from(part.len()).ok()?) - 1.0;
        sum += f64::from(v) / full;
        n += 1.0;
    }
    (n > 0.0).then(|| sum / n)
}

/// Dark when the channels average below half of full scale. `main` has already refused any
/// spec this cannot read, so the `unwrap_or` is unreachable rather than a policy.
fn is_dark(spec: &str) -> bool {
    mean_channel(spec).unwrap_or(0.0) < 0.5
}
