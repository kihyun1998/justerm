// Static file server for the `demo/*.html` proofs. Playwright's `webServer` starts it.
//
// It exists because the pages are ES modules importing a wasm-bindgen bundle: they need correct
// `Content-Type` headers (`text/javascript` for `.js`, `application/wasm` for `.wasm` — the latter
// is required for `WebAssembly.instantiateStreaming`), which `python -m http.server` does not send
// on Windows.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const PORT = Number(process.env.PORT ?? 8269);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".css": "text/css",
};

createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host}`);
  // Reject traversal outside the crate root before touching the filesystem.
  const path = join(ROOT, normalize(url.pathname).replace(/^(\.\.[/\\])+/, ""));
  if (!path.startsWith(ROOT)) {
    res.writeHead(403).end("forbidden");
    return;
  }
  try {
    const body = await readFile(path);
    res.writeHead(200, { "Content-Type": MIME[extname(path)] ?? "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(404).end("not found");
  }
}).listen(PORT, "127.0.0.1", () => {
  console.log(`serving ${ROOT} on http://127.0.0.1:${PORT}`);
});
