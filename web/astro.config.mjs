import { readdirSync, readFileSync } from "node:fs";
import starlight from "@astrojs/starlight";
import { defineConfig, passthroughImageService } from "astro/config";

// Static site: the home page and section pages, the docs (Starlight, from
// ../docs), and /app which hosts the WebAssembly editor (built into public/pkg
// by ../scripts/build-web.sh).
//
// The public address, used for canonical links, og:url, absolute preview
// images and the sitemap. Override with SITE_URL for previews or forks.
const site = process.env.SITE_URL || "https://shine.lupyd.com";
const repo = "https://github.com/ash-hashtag/shine";

/** Docs in sidebar order, from each page's `order` front matter. */
function sidebar() {
  const docs = new URL("../docs/", import.meta.url);
  return readdirSync(docs)
    .filter((f) => f.endsWith(".md") && f !== "README.md")
    .map((f) => {
      const text = readFileSync(new URL(f, docs), "utf8");
      return { slug: `docs/${f.slice(0, -3).toLowerCase()}`, order: Number(text.match(/^order:\s*(\d+)/m)?.[1] ?? 99) };
    })
    .sort((a, b) => a.order - b.order)
    .map(({ slug }) => slug);
}

/**
 * The docs link to each other as `parts.md` so they work on GitHub; on the
 * site each page lives at /docs/<name>/ (README at /docs/).
 */
function rehypeDocs() {
  const visit = (node) => {
    const href = node.type === "element" && node.tagName === "a" ? node.properties?.href : undefined;
    const m = typeof href === "string" && href.match(/^([\w-]+)\.md(#.*)?$/);
    if (m) {
      const page = m[1].toLowerCase() === "readme" ? "" : `${m[1].toLowerCase()}/`;
      node.properties.href = `/docs/${page}${m[2] ?? ""}`;
    }
    for (const child of node.children ?? []) visit(child);
  };
  return (tree) => {
    // Starlight shows the front-matter title as the page heading; the
    // Markdown's own `# Title` (there for GitHub readers) would repeat it.
    const first = tree.children?.findIndex((n) => n.type === "element");
    if (first >= 0 && tree.children[first].tagName === "h1") tree.children.splice(first, 1);
    visit(tree);
  };
}

export default defineConfig({
  site,
  output: "static",
  trailingSlash: "ignore",
  // Screenshots are already optimised WebP; serve them as they are.
  image: { service: passthroughImageService() },
  markdown: { rehypePlugins: [rehypeDocs] },
  integrations: [
    starlight({
      title: "Shine docs",
      description: "Guides and reference for Shine, the photonic circuit simulator.",
      logo: { src: "./src/assets/logo.svg", replacesTitle: false },
      favicon: "/favicon.svg",
      // Raster fallbacks for browsers without SVG icons (scripts/favicons.sh).
      head: [
        { tag: "link", attrs: { rel: "icon", href: "/favicon.ico", sizes: "48x48" } },
        { tag: "link", attrs: { rel: "icon", href: "/favicon.webp", type: "image/webp", sizes: "192x192" } },
      ],
      social: [{ icon: "github", label: "GitHub", href: repo }],
      sidebar: [
        { label: "Overview", link: "/docs/" },
        { label: "Guides", items: sidebar() },
      ],
      lastUpdated: true,
      pagination: true,
      disable404Route: true,
      customCss: ["./src/styles/starlight.css"],
      components: { SocialIcons: "./src/components/DocsHeaderLinks.astro" },
      routeMiddleware: "./src/docs-route.ts",
      expressiveCode: { themes: ["github-dark", "github-light"] },
    }),
  ],
});
