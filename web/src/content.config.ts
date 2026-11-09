import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

// The documentation is plain Markdown in ../docs, so it reads well on GitHub
// too; the site renders it at /docs and serves it to language models at
// /llms.txt and /llms-full.txt.
const docs = defineCollection({
  loader: glob({ pattern: "*.md", base: "../docs" }),
  schema: z.object({
    title: z.string(),
    description: z.string(),
    order: z.number(),
  }),
});

export const collections = { docs };
