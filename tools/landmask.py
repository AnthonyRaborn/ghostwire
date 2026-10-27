"""Regenerates src/ui/land.txt, the globe's land mask: 180x90 cells of 2 degrees each,
'#' for land and '.' for sea, rows from 90N down and columns from 180W east.

Source: Natural Earth's 1:110m land polygons (public domain),
https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_land.geojson

    python3 tools/landmask.py ne_110m_land.geojson src/ui/land.txt

Each cell is land when at least 40% of a 4x4 grid of points inside it is, so narrow
islands (Japan, the UK, New Zealand) survive at this resolution. Takes about a minute.
"""

import json
import sys

COLS, ROWS, SUB = 180, 90, 4
LAND_SHARE = 0.4


def main(src, out):
    edges = []
    for feature in json.load(open(src))["features"]:
        for ring in feature["geometry"]["coordinates"]:
            for (x1, y1), (x2, y2) in zip(ring, ring[1:] + ring[:1]):
                if y1 != y2:
                    edges.append((x1, y1, x2, y2))

    def inside(x, y):
        # Even-odd ray casting over every ring, so holes cut themselves out.
        hit = False
        for x1, y1, x2, y2 in edges:
            if (y1 > y) != (y2 > y) and x1 + (y - y1) * (x2 - x1) / (y2 - y1) > x:
                hit = not hit
        return hit

    rows = []
    for r in range(ROWS):
        line = []
        for c in range(COLS):
            hits = sum(
                inside(-180 + (c + (sx + 0.5) / SUB) * 2, 90 - (r + (sy + 0.5) / SUB) * 2)
                for sy in range(SUB)
                for sx in range(SUB)
            )
            line.append("#" if hits / SUB**2 >= LAND_SHARE else ".")
        rows.append("".join(line))
    with open(out, "w") as f:
        f.write("\n".join(rows) + "\n")


if __name__ == "__main__":
    main(*sys.argv[1:])
