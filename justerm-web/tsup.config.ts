import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm"],
  dts: true,
  clean: true,
  // WASM-backed deps stay external — the consumer's bundler instantiates them
  // (vite-plugin-wasm or equivalent). Covers the `/colors.js` subpath too.
  external: ["justerm-renderer", "justerm-wasm-decode", "justerm-wasm-decode/*"],
});
