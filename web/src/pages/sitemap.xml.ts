import type { APIRoute } from "astro";
import { docs, docPath } from "../docs";

// Pages worth indexing. /app is an interactive canvas and the 404 page has
// no content, so both are marked noindex and left out.
const fixed = [{ path: "", priority: "1.0" }];

export const GET: APIRoute = async ({ site }) => {
  const pages = [...fixed, ...(await docs()).map((d) => ({ path: docPath(d), priority: "0.7" }))];
  const base = import.meta.env.BASE_URL;
  const urls = site
    ? pages
        .map(({ path, priority }) => {
          const loc = new URL(`${base}${path}`, site).href;
          return `  <url><loc>${loc}</loc><priority>${priority}</priority></url>`;
        })
        .join("\n")
    : "";
  const xml = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls}
</urlset>
`;
  return new Response(xml, { headers: { "Content-Type": "application/xml; charset=utf-8" } });
};
