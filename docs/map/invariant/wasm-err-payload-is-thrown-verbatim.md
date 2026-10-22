# Invariant — what a wasm export puts in `Err` is thrown verbatim, so the Rust side picks the consumer's error type

## The fact

`wasm_bindgen` throws an `Err(JsValue)` payload as-is — no wrapping, no default. The JS type a consumer
catches is whatever the Rust side constructed:

| constructed as | `typeof` | `instanceof Error` | `.message` | `.stack` |
|---|---|---|---|---|
| `JsValue::from_str("BadMagic")` | `string` | `false` | `undefined` | `undefined` |
| `js_sys::Error::new("BadMagic")` | `object` | `true` | `"BadMagic"` | present |

Measured (#662) by building the package as it publishes (`wasm-pack build --target nodejs`) and catching
the throw, not by reading wasm-bindgen's docs.

So a fallible `#[wasm_bindgen]` export decides its consumers' error handling, and the wrong choice reads
as working: `throw "BadMagic"` still prints in a console and in `String(e)`; every structured use of it
disappears.

A `panic!` on a path to JS is a third shape: it crosses as a `RuntimeError` object (`instanceof Error`
true, `.message` `unreachable`), distinguishable in a `catch` and saying the wrong thing.

## Why it is cross-cutting

The construction sits in whichever crate crosses into JS and reads as local business; the contract
belongs to the published surface, frozen at publish and shared by every consumer. The two crates that
cross chose independently and differently, and neither calls the other (re-verified at HEAD 7bb31f7):

- `justerm-wasm-decode/src/lib.rs — decode_frame` (`decodeFrame`, the decoder's only fallible export)
  maps `DecodeError` through `js_sys::Error::new(&format!("{e:?}"))`.
- `justerm-renderer` still constructs `JsValue::from_str` at its fallible entry points —
  `src/webgl.rs` (no document, canvas not found, no webgl2 context, missing uniform, capacity and grid
  checks), `src/webgl/draw.rs` (`ResolveError` arms), `src/rasterizer.rs` (`█` produced no ink, no 2d
  context).

It is invisible from Rust: `Result<T, JsValue>` is the same type either way. `justerm-wasm-decode/tests/web.rs`
asserted `decode_frame(…).is_err()` since #34 and would have passed forever; the generated `.d.ts`
types no throw.

## Territories it holds in

- [wasm decoder](../territory/wasm-decoder.md) — `decode_frame` and its JS-side test in
  `justerm-wasm-decode/tests/web.rs` (`dyn_into::<js_sys::Error>()` under `wasm_bindgen_test`, wasm32
  only). Added here (new territory).
- [published surface](../territory/published-surface.md) — the thrown value is consumed by strangers
  and frozen at publish, yet nothing about it is compiled, gated or stated in a README.
- [wire format](../territory/wire-format.md) — a `DecodeError` variant name is the whole diagnostic a
  JS consumer gets for six distinct malformations (`BadSpan`'s doc-comment, #582), so where that name
  lands is the whole channel.
- [GL context lifecycle](../territory/gl-context-lifecycle.md) and
  [cell geometry](../territory/cell-geometry.md) — the renderer's fallible entry points above, still
  bare strings; #662 was scoped to the decoder, the one with a record (ADR-0008) obliging the other
  shape.

## What a violation looks like

Nothing fails anywhere. A consumer writes:

```js
try { decodeFrame(bytes); } catch (e) { report(e.message ?? String(e)); }
```

`e.message` is `undefined`, the `??` fallback prints the variant, and the defect never surfaces. Without
the fallback the report is `undefined`. A logger keyed on `e.stack` gets nothing; an
`if (e instanceof Error)` branch silently takes its `else`, usually "unknown failure" — a precisely
diagnosed error reported as unrecognised. Only a JS-side assertion on wasm32 catches it.

## Discovery history

- **#34 / ADR-0008** (2026-06-19) — recorded with the crate: "Errors throw. `DecodeError` maps to a
  thrown JS `Error` (variant name in the message)." The implementation shipped `JsValue::from_str`, and
  the two never met through eleven releases and a crate rename.
- **#582** (2026-07-30) — `BadSpan` became the diagnostic for six malformations; its doc-comment said
  "formats the variant name into the thrown value" — accurate, and not the word `Error`.
- **#662** (2026-07-31) — measured and fixed for the decoder, found by reading the doc-comment one line
  above the code. No consumer reported it or could have: the npm package had no known consumer, and
  nothing in the repo catches a decoder or renderer error.
- **#688** — the panic shape, from a dependency: `glow`'s `get_supported_extensions().unwrap()` on a lost
  GL context, which a grep of this repo for `unwrap` would not find.

## Where it will recur

- **Any new fallible `#[wasm_bindgen]` export** in any family crate — ask what a `catch` block
  receives, not whether it returns a `Result`.
- **Any `map_err` / `ok_or_else` on a path to JS.** Every renderer site is that shape and none is wrong
  read locally.
- **A new consumer.** The family's error surface is unobserved, so this costs nothing until the first
  `catch`, then all at once.
- **An external call on a path to JS** — ask what the API returns when the thing it names is gone, and
  whether the crate between treats that as a value or an invariant (#688).

Ask rather than trust a count:

```sh
rg -n 'JsValue::from_str' justerm-wasm-decode/src justerm-renderer/src
```
