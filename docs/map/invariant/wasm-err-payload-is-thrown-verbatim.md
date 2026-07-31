# Invariant — what a wasm export puts in `Err` is thrown verbatim, so the Rust side picks the consumer's error type

## The fact

`wasm_bindgen` throws an `Err(JsValue)` payload **as-is**. It does not wrap it, and there is no
default. So the JS type a consumer catches is whatever the Rust side happened to construct:

| constructed as | `typeof` | `instanceof Error` | `.message` | `.stack` |
|---|---|---|---|---|
| `JsValue::from_str("BadMagic")` | `string` | `false` | `undefined` | `undefined` |
| `js_sys::Error::new("BadMagic")` | `object` | `true` | `"BadMagic"` | present |

Measured (#662) by building the package the way it publishes — `wasm-pack build --target nodejs` —
and catching the throw, not by reading wasm-bindgen's documentation.

Therefore: **a fallible `#[wasm_bindgen]` export decides its consumers' error handling, and the
wrong choice reads as working.** `throw "BadMagic"` is a valid throw; the variant name still prints
in a console and still shows up in `String(e)`. What disappears is every *structured* use of it.

## Why it is cross-cutting

The construction sits in whichever crate crosses into JS and reads as that crate's local business.
The contract it decides does not: it belongs to the [published surface](../territory/published-surface.md),
which freezes at publish and is shared by every consumer of every family package. The two crates
that cross this boundary chose independently, and differently.

It is also **invisible from Rust**. `Result<T, JsValue>` is the same type either way, and the
difference is a JS-side property no Rust test observes: `justerm-wasm-decode/tests/web.rs` had
asserted `decode_frame(…).is_err()` since #34 and would have passed forever, because both halves
are `Err`. The generated `.d.ts` does not help either — TypeScript types no throw at all.

## Territories it holds in

- [published surface](../territory/published-surface.md) — the thrown value is part of what a
  stranger consumes and is frozen at publish, while nothing about it is compiled, gated, or stated
  in a README. It is the same class as a README's prose, one layer in.
- [wire format](../territory/wire-format.md) — `decodeFrame` is the decoder's **only** fallible
  export, and a `DecodeError` variant name is the entire diagnostic a JS consumer receives for six
  distinct malformations (see `BadSpan`'s doc-comment, #582). Where that name lands — in a
  `.message` or as the value itself — is therefore the whole channel, not a detail of it.
- [GL context lifecycle](../territory/gl-context-lifecycle.md) and
  [cell geometry](../territory/cell-geometry.md) — the renderer's fallible entry points (a missing
  canvas, no WebGL2 context, a `█` that rasterizes to no ink, a frame that does not fit its grid).
  These are **still bare strings**, and #662 did not change them: it was scoped to the decoder,
  which is the one of the two that had a record obliging the other shape.

## What a violation looks like

Nothing fails, anywhere. A consumer writes the shape every JS codebase writes:

```js
try { decodeFrame(bytes); } catch (e) { report(e.message ?? String(e)); }
```

`e.message` is `undefined`, so the `??` fallback prints the variant and the defect never surfaces.
Written without that fallback, the report is `undefined` and the diagnostic is gone. A logger keyed
on `e.stack` gets nothing. Any `if (e instanceof Error)` branch silently takes its `else`, which is
usually the "unknown failure" path — so a *precisely* diagnosed error is reported as an unrecognised
one.

The test that catches it has to assert on the **JS** side — `dyn_into::<js_sys::Error>()` inside a
`wasm_bindgen_test`, which runs on wasm32 only. A host-side `cargo test` cannot reach it, and a
Rust-side `is_err()` cannot tell the two constructions apart.

## Discovery history

- **#34 / ADR-0008** (2026-06-19) — the decision was recorded at the same time as the crate: *"Errors
  throw. `DecodeError` maps to a thrown JS `Error` (variant name in the message)."* The
  implementation shipped `JsValue::from_str`, and the two never met — through eleven releases and a
  crate rename.
- **#582** (2026-07-30) — `BadSpan` became the whole diagnostic for six malformations, and its
  doc-comment weighed that against a new variant using the phrase *"formats the variant name into
  the thrown value"*. Accurate, and notably not the word `Error`.
- **#662** (2026-07-31) — measured, and fixed for the decoder. Found by reading the doc-comment one
  line above the code; **no consumer reported it, and none could have** — the npm package has no
  known consumer, and inside this repo nothing catches a decoder or renderer error at all.

## Where it will recur

- **Any new fallible `#[wasm_bindgen]` export**, in any family crate. The question to ask is not
  "does it return a `Result`" — the compiler has that — but *what a `catch` block receives*.
- **Any `map_err` / `ok_or_else` on a path that reaches JS.** Every renderer site is that shape, and
  not one of them is wrong when read locally; the fact only exists at the boundary.
- **A new consumer.** The whole family's error surface is currently unobserved, so this class costs
  nothing today and costs it all at once — the first consumer to write a `catch` inherits every site
  at whatever shape it happens to be in.

Ask, rather than trusting a count written here:

```sh
rg -n 'JsValue::from_str' justerm-wasm-decode/src justerm-renderer/src
```
