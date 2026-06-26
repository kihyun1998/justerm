//! `justerm` has been **renamed to [`justerm-core`](https://crates.io/crates/justerm-core)**.
//!
//! This `0.5.1` release is a one-shot facade: it re-exports `justerm-core` so existing
//! `justerm = "0.5"` dependants keep compiling, while signalling the rename. It will **not**
//! be updated — depend on `justerm-core` directly. See ADR-0010 in the repository.
//!
//! ```ignore
//! // old:
//! use justerm::Engine;
//! // new:
//! use justerm_core::Engine;
//! ```

pub use justerm_core::*;
