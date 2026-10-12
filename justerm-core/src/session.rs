//! The [`Session`]: one [`Engine`] bound to one frame sink.

use crate::{Engine, TermEvent, encode};

/// Where a [`Session`] sends its encoded frames — a Tauri channel, a WebSocket, a test recorder.
/// Each frame is one [`encode`]d buffer, ready for [`decode`](crate::decode) on the other side.
pub trait FrameSink {
    /// Take one encoded frame.
    fn send_frame(&mut self, frame: Vec<u8>);
}

/// What one [`Session::pump`] hands back: everything the engine produced for the caller since the
/// last pump.
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)).** The
/// session builds it and no public function accepts one, so the attribute would bind nothing; it
/// has a `Default` for a caller that wants one anyway.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Pumped {
    /// Reply bytes (DA, DSR, …) for the caller to write back to whatever feeds the session — the
    /// PTY or the SSH channel. Nothing answers the application until they are written.
    pub replies: Vec<u8>,
    /// The engine's events, unfiltered: which of them matter, and what to do with them, is the
    /// caller's policy.
    pub events: Vec<TermEvent>,
}

/// One [`Engine`] bound to one [`FrameSink`]: the unit a consumer feeds, pumps and subscribes to
/// ([ADR-0034](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)).
///
/// It has no clock and does no I/O. The caller brings the bytes and decides when to
/// [`pump`](Session::pump): on a timer, after an ack, or in a test whenever it likes. A frame is
/// encoded only when [`Engine::frame_pending`] says the next one may differ, and damage is reset
/// as it is sent.
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)).** Its
/// fields are private, so there is no outside literal for the attribute to bind; it is built only
/// by [`Session::new`].
pub struct Session {
    engine: Engine,
    sink: Option<Box<dyn FrameSink + Send>>,
    ended: bool,
}

impl Session {
    /// A session around `engine`, with no sink yet.
    pub fn new(engine: Engine) -> Self {
        Session {
            engine,
            sink: None,
            ended: false,
        }
    }

    /// The engine, for reads: selection text, search, lines, the grid.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Change the engine's view — scroll, select, search — through `f`. On an
    /// [ended](Session::end) session the resulting frame is sent before this returns, since nothing
    /// pumps it any more; on a live one it waits for the next [`pump`](Session::pump).
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Engine) -> T) -> T {
        let out = f(&mut self.engine);
        if self.ended {
            self.flush();
        }
        out
    }

    /// Attach `sink`, replacing any previous one. It is sent a `Full` frame of the current screen at
    /// once, so a pane that attaches after output has started has a whole screen to diff from.
    pub fn subscribe(&mut self, mut sink: Box<dyn FrameSink + Send>) {
        self.engine.mark_fully_damaged();
        sink.send_frame(encode(&self.engine.frame()));
        self.engine.reset_damage();
        self.sink = Some(sink);
    }

    /// Feed VT bytes from the session's source (see [`Engine::feed`]).
    pub fn feed(&mut self, bytes: &[u8]) {
        self.engine.feed(bytes);
    }

    /// Send the pending frame, if there is one and a sink to take it, then hand back the replies and
    /// events gathered since the last pump. Both queues are emptied on every call, sink or not —
    /// the engine keeps them unbounded.
    pub fn pump(&mut self) -> Pumped {
        self.flush();
        Pumped {
            replies: self.engine.drain_replies(),
            events: self.engine.drain_events(),
        }
    }

    /// Resize the engine and return the `(cols, rows)` it adopted — the size to give the PTY or
    /// SSH channel, which can be wider than asked (see [`Engine::resize`]).
    pub fn resize(&mut self, cols: usize, rows: usize) -> (usize, usize) {
        self.engine.resize(cols, rows);
        let grid = self.engine.grid();
        (grid.cols(), grid.rows())
    }

    /// The source has ended: no pump will come, so every later [`edit`](Session::edit) sends its
    /// own frame. The engine stays readable and scrollable until the session is dropped.
    pub fn end(&mut self) {
        self.ended = true;
    }

    /// Whether [`end`](Session::end) has been called.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    fn flush(&mut self) {
        if !self.engine.frame_pending() {
            return;
        }
        if let Some(sink) = &mut self.sink {
            sink.send_frame(encode(&self.engine.frame()));
            self.engine.reset_damage();
        }
    }
}
