import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["esm"],
  dts: true,
  clean: true,
  // @beamterm/renderer (WASM) stays external — the consumer's bundler loads it.
  external: ["@beamterm/renderer"],
});
