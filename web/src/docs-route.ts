import { defineRouteMiddleware } from "@astrojs/starlight/route-data";
import { inPublic, lastModified } from "./git";

// Per-page additions to Starlight's head: the page's social card, schema.org
// data (TechArticle and breadcrumbs), and an edit link into the repository.
export const onRequest = defineRouteMiddleware((context) => {
  const route = context.locals.starlightRoute;
  const { entry, head } = route;
  const site = context.site;
  const base = import.meta.env.BASE_URL;
  const abs = (path: string) => (site ? new URL(`${base}${path}`, site).href : `${base}${path}`);

  const file = entry.id === "docs" ? "README.md" : `${entry.id.slice("docs/".length)}.md`;
  route.editUrl = new URL(`https://github.com/ash-hashtag/shine/edit/main/docs/${file}`);
  // Starlight looks up git history inside web/; the docs live one level up.
  const modified = lastModified(`docs/${file}`);
  if (modified) route.lastUpdated = new Date(modified);

  const cardName = `og/${file.replace(/\.md$/, "").toLowerCase()}.webp`;
  const card = abs(inPublic(cardName) ? cardName : "screenshots/og.webp");
  const title = entry.data.title;
  const description = entry.data.description ?? "";
  const meta = (attrs: Record<string, string>) => head.push({ tag: "meta", attrs });
  meta({ property: "og:image", content: card });
  meta({ property: "og:image:type", content: "image/webp" });
  meta({ property: "og:image:width", content: "1200" });
  meta({ property: "og:image:height", content: "630" });
  meta({ property: "og:image:alt", content: `${title}: ${description}` });
  meta({ name: "twitter:image", content: card });
  meta({ name: "twitter:image:alt", content: `${title}: ${description}` });
  meta({ name: "robots", content: "index, follow, max-image-preview:large, max-snippet:-1" });
  if (route.lastUpdated) meta({ property: "article:modified_time", content: route.lastUpdated.toISOString() });

  const pageUrl = abs(`${entry.id}/`);
  const graph = [
    {
      "@type": "TechArticle",
      "@id": `${pageUrl}#article`,
      headline: title,
      description,
      url: pageUrl,
      image: card,
      inLanguage: "en",
      ...(route.lastUpdated && { dateModified: route.lastUpdated.toISOString() }),
      isPartOf: { "@type": "WebSite", name: "Shine", url: abs("") },
      about: { "@type": "SoftwareApplication", name: "Shine", url: abs("") },
      publisher: { "@type": "Organization", name: "Lupyd", url: "https://lupyd.com" },
    },
    {
      "@type": "BreadcrumbList",
      itemListElement: [
        { "@type": "ListItem", position: 1, name: "Shine", item: abs("") },
        { "@type": "ListItem", position: 2, name: "Docs", item: abs("docs/") },
        ...(entry.id === "docs" ? [] : [{ "@type": "ListItem", position: 3, name: title, item: pageUrl }]),
      ],
    },
  ];
  head.push({
    tag: "script",
    attrs: { type: "application/ld+json" },
    content: JSON.stringify({ "@context": "https://schema.org", "@graph": graph }),
  });
  head.push({ tag: "link", attrs: { rel: "manifest", href: `${base}site.webmanifest` } });
  head.push({ tag: "link", attrs: { rel: "alternate", type: "text/plain", title: "Shine documentation for language models", href: `${base}llms.txt` } });
});
