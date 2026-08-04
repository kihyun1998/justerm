import { describe, expect, it, vi } from "vitest";
import { ContextLossRelay } from "../src/context-loss";

describe("ContextLossRelay", () => {
  it("fires the handler the consumer registered", () => {
    const relay = new ContextLossRelay();
    const handler = vi.fn();
    relay.set(handler);

    relay.notify();

    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("is silent with no handler registered", () => {
    // The relay is registered with the renderer unconditionally at `create`, so it is notified
    // whether or not the consumer opted in. A loss on a widget with no `onContextLoss` must be a
    // no-op, not a TypeError from the renderer's timer closure.
    const relay = new ContextLossRelay();
    expect(() => relay.notify()).not.toThrow();
  });

  it("stops notifying once the widget has ended", () => {
    // The dispose obligation, and it is the reference's behaviour rather than a preference:
    // xterm.js's disposable clears its restore timeout (`WebglRenderer.ts:161-163`), so a disposed
    // renderer never delivers `onContextLoss`. justerm-renderer clears its own callback slot only
    // in `Drop` (`webgl.rs`), which `Terminal.dispose()` never reaches — it stops work, not memory
    // — so the gate has to live on this side to keep the same observable contract.
    const relay = new ContextLossRelay();
    const handler = vi.fn();
    relay.set(handler);

    relay.end();
    relay.notify();

    expect(handler).not.toHaveBeenCalled();
  });

  it("cannot be revived by a handler registered after the end", () => {
    // `Terminal.dispose()` is end of life, not unmount (#606), so a late `setOnContextLoss` must
    // not resurrect the channel. Without this, `end()` would read as "clear the handler" rather
    // than "close the relay", and the two differ only here.
    const relay = new ContextLossRelay();
    const handler = vi.fn();

    relay.end();
    relay.set(handler);
    relay.notify();

    expect(handler).not.toHaveBeenCalled();
  });

  it("delivers to the current handler only, never to a replaced one", () => {
    const relay = new ContextLossRelay();
    const first = vi.fn();
    const second = vi.fn();

    relay.set(first);
    relay.set(second);
    relay.notify();

    expect(second).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
  });

  it("unregisters on `undefined`, leaving the relay itself registered", () => {
    // The consumer detaches by passing `undefined`; the renderer keeps holding `notify`, because
    // its `setOnContextLoss` takes a `Function` and has no unset. That asymmetry is the reason this
    // indirection exists at all.
    const relay = new ContextLossRelay();
    const handler = vi.fn();
    relay.set(handler);

    relay.set(undefined);
    relay.notify();

    expect(handler).not.toHaveBeenCalled();
  });

  it("keeps one stable identity across handler swaps", () => {
    // `create` registers `notify` with the renderer exactly once, so its identity must not depend
    // on which handler is current — otherwise a runtime swap would silently leave the renderer
    // holding the previous closure.
    const relay = new ContextLossRelay();
    const before = relay.notify;
    relay.set(vi.fn());

    expect(relay.notify).toBe(before);
  });

  it("still delivers once detached from the relay, which is how the renderer holds it", () => {
    // The check above cannot see this, and the two look like one property. A prototype *method*
    // also has a stable identity, so the swap test passes either way — but `setOnContextLoss`
    // stores the bare function and the renderer's timer closure calls it with no receiver, so a
    // method would lose `this` and throw inside a Rust-scheduled callback. Only calling it
    // detached separates the two implementations.
    const relay = new ContextLossRelay();
    const handler = vi.fn();
    relay.set(handler);

    const asTheRendererHoldsIt = relay.notify;
    asTheRendererHoldsIt();

    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("delivers once per notification, not once per registration", () => {
    // The renderer promises at most one notification per loss (`context_loss.rs`
    // `one_loss_notifies_the_consumer_exactly_once`); this asserts the relay adds no fan-out of
    // its own, so a consumer that re-registered mid-life still hears a single call.
    const relay = new ContextLossRelay();
    const handler = vi.fn();
    relay.set(handler);
    relay.set(handler);

    relay.notify();

    expect(handler).toHaveBeenCalledTimes(1);
  });
});
