#!/usr/bin/env python3
"""Generate site/assets/linkedin-*.png — the LinkedIn cover banners.

Reuses the committed pixel-art mark (site/assets/logo.png) unchanged and composes the
brand lockup around it, so the banner and the social preview stay the same brand.
Palette and copy are lifted from site/styles.css and site/index.html rather than
re-typed, so the banner can't drift from the landing page.

Two sizes, both at LinkedIn's documented spec:
  site/assets/linkedin-profile-cover.png  1584x396  — personal profile background
  site/assets/linkedin-page-cover.png     1128x191  — company / showcase Page cover

The left 400px (300px on the Page size) is kept clear of all content: the profile
avatar overlaps the banner's bottom-left and would otherwise cover the lockup.

Unlike tools/gen_demo_gif.py (pure PIL), this renders through headless Chrome —
the banner needs real text layout (Inter Display, negative tracking, gradients,
rounded pills) that PIL would only approximate. Deterministic and committed;
regenerate by hand, it is not wired into CI:

    python3 tools/gen_linkedin_banners.py            # -> site/assets/
    python3 tools/gen_linkedin_banners.py --no-stats # omit the 11/9 counters

Requires: google-chrome, ImageMagick, and the Inter font family.
"""
import argparse
import base64
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ASSETS = ROOT / "site" / "assets"

# ---- palette (lifted from site/styles.css) ----
INK = "#14110F"        # warm near-black page
INK2 = "#1A140F"
TERRA = "#D97757"      # primary accent
TERRA_LT = "#E8916F"
CREAM = "#FBFAF9"
MUTED = "#B3A99F"
GREEN = "#4ADE94"      # the CI-green accent
SEA = "45,106,142"     # the logo's sea blue, as rgb() parts

# ---- copy (lifted from site/index.html) ----
NAME = "Shipmates"
TAGLINE = "Stop being your AI&rsquo;s for-loop. Give it a crew. &#9875;"
SUBTITLE = "Sub-agents &amp; slash-command workflows for Claude Code"
URL = "github.com/saman-mb/shipmates"

# The stage sequence /ship-issue actually runs, abbreviated to fit.
STAGES = ["Plan", "Isolate", "Build", "CI gate", "Review", "Deliver"]
CI_STAGE = "CI gate"


def stage_rail():
    parts = []
    for i, stage in enumerate(STAGES):
        ci = stage == CI_STAGE
        dot = "<i></i>" if ci else ""
        parts.append(f'<span class="stage{" stage--ci" if ci else ""}">{dot}{stage}</span>')
        if i < len(STAGES) - 1:
            parts.append('<span class="arrow">&rarr;</span>')
    return "".join(parts)


def page(w, h, big, logo_b64, show_stats):
    """big=True renders the 1584x396 profile layout, False the 1128x191 Page one."""
    pad_l = 400 if big else 300           # avatar safe area
    mark = 92 if big else 62
    name_px = 78 if big else 52
    tag_px = 30 if big else 21

    stats = (
        '<div class="stats">'
        f'<div class="stat"><b>{len(list((ROOT / "agents").glob("*.md")))}</b>'
        "<span>specialist agents</span></div>"
        f'<div class="stat"><b>{len(list((ROOT / "commands").glob("*.md")))}</b>'
        "<span>slash-command workflows</span></div>"
        "</div>"
    ) if (big and show_stats) else ""

    subline = (
        f'<div class="sub">{SUBTITLE}<span class="dot">&middot;</span>{URL}</div>'
    ) if big else ""

    rail = f'<div class="rail">{stage_rail()}</div>' if big else ""

    return f"""<!doctype html>
<meta charset="utf-8">
<style>
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  html,body {{ width:{w}px; height:{h}px; overflow:hidden; }}
  body {{
    background:{INK};
    font-family:"Inter Display", Inter, system-ui, sans-serif;
    -webkit-font-smoothing:antialiased;
    position:relative;
  }}
  /* warm dusk glow — echoes the pixel mark's sunset without competing with it */
  .glow {{
    position:absolute; inset:0;
    background:
      radial-gradient(120% 150% at 78% 12%, rgba(217,119,87,.34) 0%, rgba(217,119,87,.10) 38%, transparent 66%),
      radial-gradient(90% 120% at 12% 96%, rgba({SEA},.30) 0%, transparent 60%),
      linear-gradient(180deg, {INK2} 0%, {INK} 62%);
  }}
  /* sea wash along the bottom edge: texture, never a dividing line */
  .sea {{
    position:absolute; left:0; right:0; bottom:0; height:{int(h * 0.16)}px;
    background:linear-gradient(180deg, rgba({SEA},0) 0%, rgba({SEA},.20) 100%);
  }}
  .wrap {{
    position:absolute; inset:0;
    padding-left:{pad_l}px; padding-right:{72 if big else 56}px;
    display:flex; flex-direction:column; justify-content:center;
    gap:{16 if big else 10}px;
  }}
  .top {{ display:flex; align-items:center; justify-content:space-between; gap:40px; }}
  .lockup {{ display:flex; align-items:center; gap:{26 if big else 18}px; }}
  .lockup img {{
    width:{mark}px; height:{mark}px; display:block;
    image-rendering:pixelated;                    /* keep the pixel art crisp */
    filter:drop-shadow(0 6px 18px rgba(0,0,0,.55));
  }}
  .name {{ font-size:{name_px}px; font-weight:800; letter-spacing:-.028em; color:{CREAM}; line-height:1; }}
  .stats {{ display:flex; gap:40px; padding-bottom:6px; }}
  .stat {{ display:flex; flex-direction:column; gap:3px; text-align:right; }}
  .stat b {{ font-size:30px; font-weight:800; color:{TERRA_LT}; line-height:1; letter-spacing:-.02em; }}
  .stat span {{ font-size:13px; font-weight:500; color:{MUTED}; letter-spacing:.01em; }}
  .tag {{ font-size:{tag_px}px; font-weight:600; letter-spacing:-.012em; color:{TERRA_LT}; line-height:1.25; }}
  .sub {{ font-size:19px; font-weight:450; color:{MUTED}; letter-spacing:-.004em; }}
  .dot {{ color:{TERRA}; padding:0 10px; }}
  /* stats hold the top-right, so the rail stays tight rather than stretched */
  .rail {{ display:flex; align-items:center; gap:14px; margin-top:{10 if big else 0}px; }}
  .stage {{
    font-size:15px; font-weight:550; color:{MUTED};
    padding:7px 14px; border-radius:999px;
    border:1px solid rgba(179,169,159,.22);
    background:rgba(255,255,255,.025);
    white-space:nowrap;
  }}
  .stage--ci {{
    color:{GREEN}; border-color:rgba(74,222,148,.34); background:rgba(74,222,148,.09);
    display:inline-flex; align-items:center; gap:8px;
  }}
  .stage--ci i {{
    width:7px; height:7px; border-radius:50%; background:{GREEN};
    box-shadow:0 0 10px rgba(74,222,148,.85); display:block;
  }}
  .arrow {{ color:rgba(179,169,159,.42); font-size:15px; }}
</style>
<div class="glow"></div>
<div class="sea"></div>
<div class="wrap">
  <div class="top">
    <div class="lockup">
      <img src="data:image/png;base64,{logo_b64}" alt="">
      <div class="name">{NAME}</div>
    </div>
    {stats}
  </div>
  <div class="tag">{TAGLINE}</div>
  {subline}
  {rail}
</div>
"""


def render(out_png, w, h, big, logo_b64, show_stats):
    html = out_png.with_suffix(".html")
    html.write_text(page(w, h, big, logo_b64, show_stats))
    try:
        # Render at 2x so text and the pixel mark stay crisp, then downsample.
        subprocess.run(
            ["google-chrome", "--headless", "--disable-gpu", "--no-sandbox",
             "--hide-scrollbars", "--force-device-scale-factor=2",
             f"--window-size={w},{h}", f"--screenshot={out_png}", f"file://{html}"],
            check=True, capture_output=True,
        )
        subprocess.run(
            ["magick", str(out_png), "-resize", f"{w}x{h}", "-strip", str(out_png)],
            check=True, capture_output=True,
        )
    finally:
        html.unlink(missing_ok=True)
    print(f"{out_png.relative_to(ROOT)}  {w}x{h}")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--no-stats", action="store_true",
                    help="omit the crew/orders counters from the profile banner")
    args = ap.parse_args()

    logo = ASSETS / "logo.png"
    if not logo.is_file():
        sys.exit(f"missing {logo} — the banner reuses the committed pixel-art mark")
    logo_b64 = base64.b64encode(logo.read_bytes()).decode()

    render(ASSETS / "linkedin-profile-cover.png", 1584, 396, True, logo_b64, not args.no_stats)
    render(ASSETS / "linkedin-page-cover.png", 1128, 191, False, logo_b64, False)


if __name__ == "__main__":
    main()
