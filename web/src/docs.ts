import { getCollection, type CollectionEntry } from "astro:content";

export type Doc = CollectionEntry<"docs">;

/** The docs in reading order; README (id `docs`) is the index page. */
export async function docs(): Promise<Doc[]> {
  return (await getCollection("docs")).sort((a, b) => a.data.order - b.data.order);
}

/** URL of a doc page relative to the site base, e.g. `docs/parts/`. */
export function docPath(doc: Doc): string {
  return `${doc.id}/`;
}

/** The page's Markdown file name in the repository's docs/ folder. */
export function docFile(doc: Doc): string {
  return doc.id === "docs" ? "README.md" : `${doc.id.slice("docs/".length)}.md`;
}

/** Name of the page's social card in public/og (scripts/og-cards.py). */
export function docCard(doc: Doc): string {
  return `og/${docFile(doc).replace(/\.md$/, "").toLowerCase()}.webp`;
}
