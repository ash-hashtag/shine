import { defineConfig } from "astro/config";

// Static site: the home page, and /app which hosts the WebAssembly editor
// (built into public/pkg by ../scripts/build-web.sh).
export default defineConfig({
  output: "static",
  trailingSlash: "ignore",
});
