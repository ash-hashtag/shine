import { getCollection, type CollectionEntry } from "astro:content";

export type Doc = CollectionEntry<"docs">;

/** The docs in reading order; README is the index page. */
export async function docs(): Promise<Doc[]> {
  return (await getCollection("docs")).sort((a, b) => a.data.order - b.data.order);
}

/** URL of a doc page relative to the site base: README is /docs/. */
export function docPath(doc: Doc): string {
  return doc.id === "readme" ? "docs/" : `docs/${doc.id}/`;
}
