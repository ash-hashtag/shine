import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";

// Astro builds from web/; resolve paths from there (bundled code has no
// reliable import.meta.url back to the sources).
const repo = resolve(process.cwd(), "..");

/** Whether a file exists under web/public. */
export function inPublic(path: string): boolean {
  return existsSync(join(process.cwd(), "public", path));
}

/**
 * When a file (relative to the repository root) last changed, from git, as
 * an ISO date. Undefined outside a git checkout or for files never committed.
 */
export function lastModified(path: string): string | undefined {
  try {
    const out = execFileSync("git", ["log", "-1", "--format=%cI", "--", path], { cwd: repo, encoding: "utf8" }).trim();
    return out || undefined;
  } catch {
    return undefined;
  }
}

/** The newest of several files' modification dates. */
export function newest(paths: string[]): string | undefined {
  return paths.map(lastModified).filter((d): d is string => !!d).sort().at(-1);
}

/** The released version, for structured data. */
export function version(): string | undefined {
  try {
    // The newest release tag: Cargo.toml runs ahead while the next version is built.
    const tag = execFileSync("git", ["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"], { cwd: repo, encoding: "utf8" });
    return tag.trim().replace(/^v/, "") || undefined;
  } catch {
    return undefined;
  }
}
