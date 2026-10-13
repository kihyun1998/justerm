# Checking another OS before CI

**CI is the guarantee; this is the fast feedback.** `.github/workflows/test.yml` runs `justerm-pty`'s
real-process tests on Linux (`test`), macOS and Windows (`pty`) for every PR. That holds whoever made
the change and on whatever machine: a Dependabot PR or a change from another checkout reaches no
local check. Keep that set. What this file adds is how to see an OS **before** CI does, from whatever
machine you are on. A red that a local run would have shown is a full CI round trip saved.

Only `justerm-pty` needs this. Everything else in the workspace is OS-independent: the engine does
no I/O.

## Why one OS is not enough

A pseudo-terminal behaves differently on each OS, and `justerm-pty`'s tests are built around that.
Read the `pty` territory note (`docs/map/territory/pty.md`) for the measurements. Here is what each OS
proves that the others cannot:

| OS | What only it shows |
|---|---|
| Windows (ConPTY) | conhost asks `ESC[6n` before the child may run: the reader's write-back, and in Raw mode its own start-up answer. Exit may come without EOF |
| Linux / macOS | the reply paths end to end. Two tests are `#[cfg(unix)]`, because conhost answers a child's queries itself. Also SIGHUP→SIGKILL escalation |
| macOS | its own timing: #1046's resize test raced there and nowhere else |

## Per host

### On Windows

- **Windows:** natively, `cargo test -p justerm-pty`.
- **Linux:** via WSL, `Ubuntu-24.04`. Setup, once:
  - **Toolchain.** In user space, at the version `rust-toolchain.toml` pins; no sudo needed:
    `wsl -d Ubuntu-24.04 -e bash -lc 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain <channel> --no-modify-path'`
  - **Linker.** Rust on Linux links through `cc`. It needs sudo, so the maintainer runs it:
    `wsl -d Ubuntu-24.04 -- sudo apt install -y build-essential`.

  Then run the tests:

  ```sh
  wsl -d Ubuntu-24.04 -e bash -lc 'cd /mnt/d/github/<worktree> && CARGO_TARGET_DIR=$HOME/jt-target ~/.cargo/bin/cargo test -p justerm-pty'
  ```

  `CARGO_TARGET_DIR` keeps the Linux build out of the worktree's `target/`. The Windows build lives
  there; sharing it would make the two rebuild each other's artifacts, and `/mnt` is slow.
- **macOS:** CI only.

### On macOS

- **macOS:** natively, `cargo test -p justerm-pty`.
- **Linux:** any Linux with a Rust toolchain and `cc`: a container (`docker run --rm -v "$PWD":/w -w /w
  rust:<channel> cargo test -p justerm-pty`) or a VM.
- **Windows:** CI only.

### On Linux

- **Linux:** natively.
- **macOS and Windows:** CI only.

## Mutation runs on another OS

A mutation proof for a `#[cfg(unix)]` test has to run where the test runs. From Windows, edit the
source on the Windows side and run the test command in WSL. The worktree is the same files under
`/mnt`. Restore the file in a `finally`, and check `git status` is clean afterwards. #1046's Unix
mutations ran this way.

## The VM is not a build host

The RHEL VM (`ssh justerm-vm`, used for PTY captures) has no `gcc` and no Rust. Installing them there
is a change to a shared machine, so ask first. WSL is the default Linux host on Windows.
