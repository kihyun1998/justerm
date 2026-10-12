# justerm-pty

Runs a command in a pseudo-terminal and drives a [`justerm-core`](https://crates.io/crates/justerm-core)
`Session` from it: the read loop, the pump timer and exit detection. It is part of the
[`justerm`](https://github.com/kihyun1998/justerm) family, and the one crate in it that does I/O.

- **Linux, macOS and Windows** (ConPTY), through [`portable-pty`](https://crates.io/crates/portable-pty).
- **No async runtime.** It reads on a dedicated thread and pumps on another, so you bridge its callbacks
  into whatever runtime you use.
- **Policy stays yours.** You choose the command, environment and directory. You build the engine and
  subscribe the frame sink. You decide what to do with each event.
- **Input and resize.** `Pty::write` queues bytes for the child. `Pty::resize` tells the child the
  size the engine actually adopted.
- **Encodings.** For a child that does not write UTF-8, inject a decoder and an encoder through
  `Options`. The engine itself reads UTF-8 only.
- **State beside the engine.** `Options::on_output` sees exactly the bytes the engine is fed, and
  `Options::on_tick` runs just before each pump.
- **Raw mode.** `Pty::spawn_raw` runs the child with no engine and hands its output to your own
  terminal (a `RawSink`). Your terminal answers the child's queries, and your sink can pause reading
  until `Pty::resume`.
