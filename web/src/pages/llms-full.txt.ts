import type { APIRoute } from "astro";
import { docs, docPath } from "../docs";

// The whole documentation as Markdown in one file, for language models.
export const GET: APIRoute = async ({ site }) => {
  const base = import.meta.env.BASE_URL;
  const url = (path: string) => (site ? new URL(`${base}${path}`, site).href : `${base}${path}`);
  const pages = await docs();
  // Relative links in the Markdown point at sibling files; make them absolute.
  const images = "https://raw.githubusercontent.com/ash-hashtag/shine/main/docs/img/";
  const absolute = (body: string) =>
    body
      .replace(/\]\(img\//g, `](${images}`)
      .replace(/\]\(([\w-]+)\.md(#[^)]*)?\)/g, (_, name: string, hash = "") =>
        `](${url(name.toLowerCase() === "readme" ? "docs/" : `docs/${name}/`)}${hash})`,
      );
  const text = pages
    .map((d) => `<!-- ${url(docPath(d))} -->\n\n${absolute((d.body ?? "").trim())}\n`)
    .join("\n---\n\n");
  return new Response(text, { headers: { "Content-Type": "text/plain; charset=utf-8" } });
};
