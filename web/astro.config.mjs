import { satteri } from "@astrojs/markdown-satteri";
import { defineConfig, passthroughImageService } from "astro/config";

// Static site: the home page, the docs (rendered from ../docs), and /app which
// hosts the WebAssembly editor (built into public/pkg by ../scripts/build-web.sh).
//
// The public address, used for canonical links, og:url, absolute preview
// images and the sitemap. Override with SITE_URL for previews or forks.
const site = process.env.SITE_URL || "https://shine.lupyd.com";

/**
 * The docs link to each other as `parts.md` so they work on GitHub; on the
 * site each page lives at /docs/<name>/ (README at /docs/).
 */
const docLinks = {
  name: "doc-links",
  element: {
    filter: ["a"],
    visit(node, ctx) {
      const href = node.properties?.href;
      const m = typeof href === "string" && href.match(/^([\w-]+)\.md(#.*)?$/);
      if (m) {
        const page = m[1].toLowerCase() === "readme" ? "" : `${m[1]}/`;
        ctx.setProperty(node, "href", `/docs/${page}${m[2] ?? ""}`);
      }
    },
  },
};

export default defineConfig({
  site,
  output: "static",
  trailingSlash: "ignore",
  // Screenshots are already optimised WebP; serve them as they are.
  image: { service: passthroughImageService() },
  markdown: { processor: satteri({ hastPlugins: [docLinks] }) },
});
