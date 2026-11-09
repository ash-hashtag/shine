import type { APIRoute } from "astro";
import { docCard, docFile, docPath, docs } from "../docs";
import { inPublic, lastModified, newest } from "../git";

interface Page {
  path: string;
  priority: string;
  /** When the content last changed (from git). */
  modified?: string;
  /** Images worth indexing with the page, relative to the site root. */
  images: { path: string; caption: string }[];
}

const xml = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

// Pages worth indexing. /app is an interactive canvas and the 404 page has
// no content, so both are marked noindex and left out.
export const GET: APIRoute = async ({ site }) => {
  const base = import.meta.env.BASE_URL;
  const home: Page = {
    path: "",
    priority: "1.0",
    modified: newest(["web/src/pages/index.astro", "web/public/screenshots"]),
    images: [
      { path: "screenshots/link.webp", caption: "A 4-channel WDM optical link: bits on four wavelengths share one waveguide." },
      { path: "screenshots/full-adder.webp", caption: "An electro-optic full adder built from free-space parts." },
      { path: "screenshots/accumulator.webp", caption: "A photonic accumulator: a register and eight full adders." },
      { path: "screenshots/components.webp", caption: "A Mach–Zehnder interferometer reused as a component." },
      { path: "screenshots/free-space.webp", caption: "A free-space Mach–Zehnder interferometer with beams and readings." },
      { path: "screenshots/dft.webp", caption: "An MZI mesh computing an optical Fourier transform." },
      { path: "screenshots/wdm.webp", caption: "The spectrum of a four-channel ring demultiplexer." },
    ],
  };
  const sections: Page[] = [
    { name: "features", caption: "Shine features", shot: "screenshots/full-adder.webp" },
    { name: "library", caption: "Shine as a Rust library", shot: "screenshots/dft.webp" },
    { name: "ai", caption: "Shine for AI assistants", shot: "screenshots/components.webp" },
  ].map(({ name, caption, shot }) => ({
    path: `${name}/`,
    priority: "0.8",
    modified: lastModified(`web/src/pages/${name}.astro`),
    images: [
      ...(inPublic(`og/site-${name}.webp`) ? [{ path: `og/site-${name}.webp`, caption }] : []),
      { path: shot, caption },
    ],
  }));
  const pages: Page[] = [
    home,
    ...sections,
    ...(await docs()).map((d) => {
      const card = docCard(d);
      return {
        path: docPath(d),
        priority: d.id === "docs" ? "0.8" : "0.7",
        modified: lastModified(`docs/${docFile(d)}`),
        images: inPublic(card) ? [{ path: card, caption: d.data.title }] : [],
      };
    }),
  ];
  const url = (path: string) => (site ? new URL(`${base}${path}`, site).href : `${base}${path}`);
  const entries = site
    ? pages.map((p) =>
        [
          "  <url>",
          `    <loc>${xml(url(p.path))}</loc>`,
          ...(p.modified ? [`    <lastmod>${p.modified}</lastmod>`] : []),
          `    <priority>${p.priority}</priority>`,
          ...p.images.map(
            (i) => `    <image:image><image:loc>${xml(url(i.path))}</image:loc><image:caption>${xml(i.caption)}</image:caption></image:image>`,
          ),
          "  </url>",
        ].join("\n"),
      )
    : [];
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:image="http://www.google.com/schemas/sitemap-image/1.1">
${entries.join("\n")}
</urlset>
`;
  return new Response(body, { headers: { "Content-Type": "application/xml; charset=utf-8" } });
};
