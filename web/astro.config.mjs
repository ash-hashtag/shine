import { defineConfig } from "astro/config";

// Static site: the home page, and /app which hosts the WebAssembly editor
// (built into public/pkg by ../scripts/build-web.sh).
//
// SITE_URL is the public address, e.g. https://shine.example.com. It makes
// canonical links, og:url, absolute preview images and the sitemap possible.
const site = process.env.SITE_URL || undefined;
if (!site) {
  console.warn("SITE_URL is not set: building without canonical URLs, og:url or a sitemap.");
}

export default defineConfig({
  site,
  output: "static",
  trailingSlash: "ignore",
});
