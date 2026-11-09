import type { APIRoute } from "astro";
import { docs, docPath } from "../docs";

// https://llmstxt.org: a short index pointing language models at the docs.
export const GET: APIRoute = async ({ site }) => {
  const base = import.meta.env.BASE_URL;
  const url = (path: string) => (site ? new URL(`${base}${path}`, site).href : `${base}${path}`);
  const pages = await docs();
  const lines = [
    "# Shine",
    "",
    "> Shine designs and simulates photonic circuits: free-space optics, waveguide components, and the electronics that drive and read them. Light is solved exactly (interference, resonances, several wavelengths); electronics are simple logic, registers and function cells. It runs in the browser, as a desktop app, as a Rust library, and as an MCP server.",
    "",
    `Everything below in one file: ${url("llms-full.txt")}`,
    "",
    "## Docs",
    "",
    ...pages.map((d) => `- [${d.data.title}](${url(docPath(d))}): ${d.data.description}`),
    "",
    "## Optional",
    "",
    `- [Web app](${url("app/")}): the editor; ?example=<name> opens an example`,
    "- [Source code](https://github.com/ash-hashtag/shine)",
    "",
  ];
  return new Response(lines.join("\n"), { headers: { "Content-Type": "text/plain; charset=utf-8" } });
};
