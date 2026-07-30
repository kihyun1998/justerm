import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Behavior tests run in plain node: the renderer (wasm/WebGL2) sits behind the
    // Renderer port, so the wiring logic is testable with a FakeRenderer and
    // needs no DOM. The real adapter is verified by `pnpm test:e2e`, which boots the
    // demo and drives real wasm in a browser (CI-wired at #341). Both halves of that
    // sentence used to say otherwise: the crate behind the port was beamterm until
    // #273, and the browser check was a manual harness until #341.
    environment: "node",
    globals: true,
    // `*.test.ts` only, deliberately: `test/published-seam.types.ts` is a type-level
    // gate with nothing to execute at runtime, checked by `pnpm typecheck` instead
    // (#646). Widening this to `test/**/*.ts` makes vitest fail on it with "No test
    // suite found in file".
    include: ["test/**/*.test.ts"],
  },
});
