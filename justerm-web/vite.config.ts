import { defineConfig } from "vite";
// justerm-renderer + justerm-wasm-decode ship the wasm-bindgen "bundler" target,
// which does `import * as wasm from "*.wasm"` (ESM integration). Vite needs these two
// plugins to instantiate that: wasm() turns the .wasm import into a real
// module, topLevelAwait() lowers the top-level `await` it (and our demo) use.
// Mirrors penterm's vite.config (rust-terminal-engine issue 11).
import topLevelAwait from "vite-plugin-top-level-await";
import wasm from "vite-plugin-wasm";

export default defineConfig({
  // Serve the manual S1 harness; src/ lives one level up.
  root: "demo",
  plugins: [wasm(), topLevelAwait()],
  // esbuild's dep pre-bundle can't follow the wasm-bindgen `import * as wasm`
  // ESM import (it produces a broken bundle that crashes on __wbindgen_start) —
  // exclude both WASM-backed packages so vite-plugin-wasm handles them instead.
  // Excluding the bare package name also covers its subpaths (e.g. the
  // `justerm-wasm-decode/colors.js` import in render-core.ts).
  optimizeDeps: {
    exclude: ["justerm-renderer", "justerm-wasm-decode"],
  },
  // demo/ imports from ../src; allow Vite to read the package root.
  server: { fs: { allow: [".."] } },
  // top-level-await emits modern syntax (async destructuring); don't let
  // esbuild try to down-level it to es2020 (its default), which fails.
  build: { target: "esnext" },
  esbuild: { target: "esnext" },
});
