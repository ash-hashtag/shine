import { defineConfig } from "astro/config";

// Static site: the home page, and /app which hosts the WebAssembly editor
// (built into public/pkg by ../scripts/build-web.sh).
//
// The public address, used for canonical links, og:url, absolute preview
// images and the sitemap. Override with SITE_URL for previews or forks.
const site = process.env.SITE_URL || "https://shine.lupyd.com";

export default defineConfig({
  site,
  output: "static",
  trailingSlash: "ignore",
});
