import { defineCollection } from "astro:content";
import { docsSchema } from "@astrojs/starlight/schema";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

// The documentation is plain Markdown in ../docs, so it reads well on GitHub
// too. Starlight renders it at /docs (README is the index); the site also
// serves it to language models at /llms.txt and /llms-full.txt.
const docs = defineCollection({
  loader: glob({
    pattern: "*.md",
    base: "../docs",
    generateId: ({ entry }) => {
      const name = entry.replace(/\.md$/, "").toLowerCase();
      return name === "readme" ? "docs" : `docs/${name}`;
    },
  }),
  schema: docsSchema({ extend: z.object({ order: z.number() }) }),
});

export const collections = { docs };
