#!/usr/bin/env python3
"""Social preview cards (1200×630 WebP) for the home page and every docs page.

    scripts/og-cards.py

Each card shows the page title, a summary and a screenshot: the home page uses
the WDM link; a docs page uses its first image (or the WDM link). Writes
web/public/screenshots/og.webp and web/public/og/<page>.webp.
"""
import html
import re
import subprocess
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parent.parent
docs = root / "docs"
template = (root / "scripts" / "og-card.html").read_text()
out_dir = root / "web" / "public" / "og"
out_dir.mkdir(parents=True, exist_ok=True)


def front_matter(text: str) -> dict[str, str]:
    block = text.split("---", 2)[1]
    return {k.strip(): v.strip() for k, v in (line.split(":", 1) for line in block.strip().splitlines())}


def width(image: Path) -> int:
    return int(subprocess.run(["magick", "identify", "-format", "%w", image], capture_output=True, text=True, check=True).stdout)


def render(kicker: str, title: str, subtitle: str, screenshot: Path, out: Path) -> None:
    page = (
        template.replace("{{KICKER}}", html.escape(kicker))
        .replace("{{TITLE}}", html.escape(title))
        .replace("{{SUBTITLE}}", html.escape(subtitle))
        .replace("{{SCREENSHOT}}", screenshot.as_uri())
    )
    with tempfile.TemporaryDirectory() as tmp:
        card = Path(tmp) / "card.html"
        card.write_text(page)
        png = Path(tmp) / "card.png"
        subprocess.run(
            ["chromium", "--headless=new", "--no-sandbox", "--hide-scrollbars", "--force-color-profile=srgb",
             "--allow-file-access-from-files", "--window-size=1200,630", f"--screenshot={png}", card.as_uri()],
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        subprocess.run(["magick", png, "-define", "webp:lossless=true", "-define", "webp:method=6", out], check=True)
    print(f"  {out.relative_to(root)}")


hero = root / "web" / "public" / "screenshots" / "link.webp"
render("", "Design photonic circuits and watch the light.",
       "Exact simulation, reusable components, in your browser.", hero, root / "web" / "public" / "screenshots" / "og.webp")

for page in sorted(docs.glob("*.md")):
    text = page.read_text()
    meta = front_matter(text)
    # The page's first full-window screenshot; small crops would look blurry.
    wide = [docs / m for m in re.findall(r"!\[[^\]]*\]\((img/[^)]+)\)", text) if width(docs / m) >= 1200]
    screenshot = wide[0] if wide else docs / "img" / "wdm-link.webp"
    # Long summaries are cut at a sentence or word boundary.
    summary = meta["description"]
    if len(summary) > 110:
        summary = summary[:110].rsplit(" ", 1)[0].rstrip(",;:") + "…"
    slug = page.stem.lower()
    render("Shine docs", meta["title"], summary, screenshot, out_dir / f"{slug}.webp")

