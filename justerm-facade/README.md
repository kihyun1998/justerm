# justerm — renamed to `justerm-core`

This crate was renamed. **Use [`justerm-core`](https://crates.io/crates/justerm-core) instead.**

```toml
# old
justerm = "0.5"
# new
justerm-core = "0.6"
```

`0.5.1` is a one-shot facade that re-exports `justerm-core` so existing `justerm = "0.5"` dependants
keep compiling. It will not be updated. The rename rationale (and why this is a facade rather than a
yank) is recorded in
[ADR-0010](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0010-all-prefixed-crate-naming.md).

Dual-licensed under Apache-2.0 or MIT, at your option.
