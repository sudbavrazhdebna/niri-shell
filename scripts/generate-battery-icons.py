#!/usr/bin/env python3
"""Write the Classic battery SVGs, from 10 to 100%, with and without charging.

Run from any directory. Colours retain the original warning levels: red through
20%, orange through 30%, then white on battery or green on AC.
"""

from pathlib import Path


DEST = Path(__file__).resolve().parents[1] / "icons/scalable/status"
WHITE = "#f5f5f7"
OUTLINE = "#ffffff"
BOLT = "M17.5 6.8 11.5 17.2H16L14.5 25.2 21 14.6H16.5Z"


def battery(level: int, charging: bool) -> str:
    colour = (
        "#ff453a" if level <= 20 else
        "#ff9500" if level <= 30 else
        "#5cdb87" if charging else WHITE
    )
    mask = f'''  <defs>
    <mask id="bolt-gap" maskUnits="userSpaceOnUse" x="0" y="0" width="32" height="32">
      <rect width="32" height="32" fill="white"/>
      <path d="{BOLT}" fill="black" stroke="black" stroke-width="1.9" stroke-linejoin="round"/>
    </mask>
  </defs>
''' if charging else ""
    bolt = f'''  <path d="{BOLT}" fill="{WHITE}" stroke="{WHITE}" stroke-width="0.35" stroke-linejoin="round"/>
''' if charging else ""
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">
{mask}  <g{' mask="url(#bolt-gap)"' if charging else ''}>
    <rect x="2" y="8.25" width="25" height="15.5" rx="4" fill="none" stroke="{OUTLINE}" stroke-width="1.5"/>
    <rect x="29" y="13" width="1.8" height="6" rx="0.9" fill="{OUTLINE}"/>
    <rect x="4.8" y="11.05" width="{19.4 * level / 100:.2f}" height="9.9" rx="1.8" fill="{colour}"/>
  </g>
{bolt}</svg>
'''


if __name__ == "__main__":
    for charging in (False, True):
        for level in range(10, 101, 10):
            kind = "charging-" if charging else ""
            (DEST / f"nshell-battery-{kind}{level}.svg").write_text(battery(level, charging))
