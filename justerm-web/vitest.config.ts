import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Behavior tests run in plain node: beamterm (WASM/WebGL) sits behind the
    // Renderer port, so the wiring logic is testable with a FakeRenderer and
    // needs no DOM. The real beamterm adapter is verified by a manual harness.
    environment: "node",
    globals: true,
    include: ["test/**/*.test.ts"],
  },
});
