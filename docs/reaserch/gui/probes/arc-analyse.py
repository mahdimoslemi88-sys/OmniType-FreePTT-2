#!/usr/bin/env python3
"""
Decides whether the reported white halo is present in a before/after pair.

# KNOWN LIMITATION - read before trusting a "not reproduced" answer
#
# This script has never returned a positive. It has also never been shown to be
# able to, because the only drag sequence that could be automated ends with the
# orb *overlapping the rect it started in*, and then "bright pixels inside the
# old rect" is just the orb's own body. So a negative from it is not evidence
# that the halo is gone.
#
# What it needs to become a real test:
#   1. a drag that ends far from where it started, so the vacated rect is empty
#      of the orb (one long move, not four short ones around the edges), and
#   2. a positive control - a screenshot pair where the halo is known to be
#      present - to prove the thresholds fire at all.
# Neither exists yet. Until they do, treat the halo as UNCONFIRMED rather than
# fixed or unfixed.
#
# The first version of this script matched the halo's measured colour
# (215,229,243) and found 21,693 pixels in a screenshot of an ordinary
# light-themed desktop - i.e. it was measuring the *application*, not the
# artifact, and would have reported "arc found" for almost any pair of
# screenshots. That is the blind-probe failure this project has hit twice
# before, so the discriminator here is structural instead:

    a leftover is bright in `after`, was not bright in `before`, and sits
    inside the window rect the orb used to own.

The third condition is what makes it a halo rather than the orb itself: the
orb legitimately paints new pixels at its new position, and only pixels in the
*old* rect cannot be explained by the orb being somewhere else.

Usage:
    python arc-analyse.py before.png after.png OLDX OLDY SIDE

The last line is always `VERDICT: <token>` so a script can read the answer
instead of parsing prose. Tokens:
    REPRODUCED  a thin bright arc inside the old rect that was not there before
    NOT_AN_ARC  bright leftovers, but too thick or scattered to be the halo
    CLEAN       nothing new and bright inside the old rect
    INCONCLUSIVE the inputs could not be compared at all
Run halo-selftest.py before trusting any of these.
"""
import sys
import numpy as np
from PIL import Image

BRIGHT = 200          # min channel that counts as "bright" (the arc peaks at 255)
MIN_BLOB = 250        # px; below this it is image noise or a single window edge


def load(p):
    return np.asarray(Image.open(p).convert("RGB")).astype(np.int16)


def bright(img):
    return img.min(axis=2) >= BRIGHT


def main():
    if len(sys.argv) < 6:
        print(__doc__)
        return 2
    before = load(sys.argv[1])
    after = load(sys.argv[2])
    ox, oy, side = (int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]))

    if before.shape != after.shape:
        print(f"  !! different sizes: {before.shape} vs {after.shape}")
        print("VERDICT: INCONCLUSIVE")
        return 1
    h, w = before.shape[:2]

    print(f"-- old window rect: ({ox},{oy}) {side}x{side}px;  screen {w}x{h}")
    print(f"-- bright = min channel >= {BRIGHT}")

    # The rect the orb used to own, clipped to the screen.
    x0, y0 = max(0, ox), max(0, oy)
    x1, y1 = min(w, ox + side), min(h, oy + side)
    if x1 - x0 < 4 or y1 - y0 < 4:
        print("  !! the old rect is (almost) off-screen; nothing to compare")
        print("VERDICT: INCONCLUSIVE")
        return 1

    mb, ma = bright(before), bright(after)
    new_in_old = ma[y0:y1, x0:x1] & ~mb[y0:y1, x0:x1]
    n = int(new_in_old.sum())
    print(f"-- new bright pixels inside the OLD rect: {n}")
    if n < MIN_BLOB:
        print(f"   -> below {MIN_BLOB}: no leftover halo in the old window rect")
        print("VERDICT: CLEAN")
        return 0

    # Where inside the old rect, and how thick? A halo is a thin curved band;
    # a solid block means something else was repainted.
    ys, xs = np.nonzero(new_in_old)
    print(f"   bbox inside rect: x[{xs.min()}..{xs.max()}] y[{ys.min()}..{ys.max()}]")
    rows = np.bincount(ys, minlength=y1 - y0)
    thick = rows[rows > 0]
    print(f"   rows touched: {len(thick)} of {y1-y0}; "
          f"median pixels per touched row: {int(np.median(thick))}")

    # Fit a circle to the new pixels: a halo of one radius fits tightly, a
    # rectangle or a blob does not.
    gx = xs + x0
    gy = ys + y0
    A = np.column_stack([gx.astype(float), gy.astype(float), np.ones(len(gx))])
    b = -(gx.astype(float) ** 2 + gy.astype(float) ** 2)
    (dd, ee, ff), *_ = np.linalg.lstsq(A, b, rcond=None)
    cx, cy = -dd / 2, -ee / 2
    r = float(np.sqrt(max(cx * cx + cy * cy - ff, 0.0)))
    resid = np.abs(np.sqrt((gx - cx) ** 2 + (gy - cy) ** 2) - r)
    p90 = float(np.percentile(resid, 90))
    print(f"   circle fit: centre=({cx:.0f},{cy:.0f}) radius={r:.1f}px "
          f"90th-pct residual={p90:.2f}px")
    if p90 <= 8.0:
        print("   -> a clean arc: THE HALO IS REPRODUCED")
        print("VERDICT: REPRODUCED")
    else:
        print("   -> too thick/scattered for a single arc; not the measured halo")
        print("VERDICT: NOT_AN_ARC")
    return 0


if __name__ == "__main__":
    sys.exit(main())
