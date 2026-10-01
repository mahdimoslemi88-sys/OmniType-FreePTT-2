#!/usr/bin/env python3
"""
Proves `arc-analyse.py` can actually answer, before anyone trusts its silence.

Why this file exists
--------------------
`arc-analyse.py` has never returned a positive, and nothing proved it *could*.
The project's rule — "a healthy tool is not the same as a valid result" — has
already bitten this investigation three times: the first analyser matched the
measured halo colour and found 21,693 pixels of an ordinary light-themed
application; the drag sequence could only end with the orb overlapping its own
old rect; and the mutation harness once reported its own blindness as a result.

So the controls come first. This script builds screenshot pairs where the answer
is known, runs the real analyser on them, and fails loudly if it disagrees:

  1. POSITIVE  - a thin arc at the measured radius/thickness/colour.
                 MUST report REPRODUCED, and the fitted radius must land near
                 the radius that was drawn. A negative from a tool that cannot
                 fire on a known halo is worthless, so this is the gate.
  2. NEGATIVE  - the same pair with no arc at all. MUST report CLEAN. Guards
                 against the analyser simply always saying yes.
  3. DISC      - a filled disc of the orb's own painted radius, overlapping the
                 old rect. This is exactly what a drag that fails to vacate the
                 rect leaves behind. MUST NOT report REPRODUCED. A disc fits a
                 circle perfectly, so a circle-fit-only test calls this a "clean
                 arc" — this control is what caught that.
  4. DECOYS    - large bright UI blocks painted in the halo's *exact measured
                 colour* (214,228,242). This is the first analyser, which found
                 21,693 such pixels and called them the artifact. MUST NOT
                 report REPRODUCED.

Every number below comes from GUI-WINDOW-ARTIFACT-REPORT.md section 15.1
(radius 120.4 px, 10 px thick, colour (214,228,242) with (255,255,255) on the
last row, background 25, on the 125% display).

Usage:
    python halo-selftest.py            # run all controls
    python halo-selftest.py --keep DIR # also write the synthetic pairs out

Exit code 0 = the analyser is trustworthy for this run; 1 = it is not, and any
hunt result from it must be reported as INCONCLUSIVE.
"""
import argparse
import os
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ANALYSER = os.path.join(HERE, "arc-analyse.py")

# --- measured facts (GUI-WINDOW-ARTIFACT-REPORT.md 15.1) ----------------------
HALO_RADIUS_PX = 120.4
HALO_THICK_PX = 10
HALO_COLOUR = (214, 228, 242)
HALO_EDGE_COLOUR = (255, 255, 255)
BACKGROUND = 25
SCREEN = (1920, 1080)
ORB_SIDE_PX = 298
# The orb's own idle painted reach, 60.89 pt at 125% DPI.
ORB_PAINTED_RADIUS_PX = 76
# The orb's IDLE palette (gui/orb_palette.rs): core PEARL_WHITE, rim/glow
# SOFT_SILVER. Both are well above the analyser's bright threshold, which is the
# whole reason a drag that fails to vacate the rect is a false-positive source.
# An earlier version of this control used the fallback icon's teal (38,198,178),
# which is *not* bright by the analyser's own definition - so the control found
# zero pixels and proved nothing while appearing to pass.
ORB_IDLE_CORE = (255, 255, 255)
ORB_IDLE_RIM = (220, 230, 240)


def canvas():
    return np.full((SCREEN[1], SCREEN[0], 3), BACKGROUND, dtype=np.uint8)


def arc_image():
    """A dark desktop with the measured halo drawn in it."""
    img = canvas()
    cx, cy = 1511, 574
    ys, xs = np.mgrid[0 : SCREEN[1], 0 : SCREEN[0]]
    r = np.sqrt((xs - cx) ** 2 + (ys - cy) ** 2)
    # Only the top cap is visible in the report; drawing the whole ring would
    # make the control easier than reality.
    upper = ys < cy
    for i in range(HALO_THICK_PX):
        band = (r >= HALO_RADIUS_PX - i) & (r < HALO_RADIUS_PX + 1) & upper
        colour = (
            HALO_COLOUR if i < HALO_THICK_PX - 1 else HALO_EDGE_COLOUR
        )
        img[band] = colour
    return img


def disc_image(offset=0):
    """A dark desktop with a filled disc — the orb's body, in its real colours.

    `offset` shifts the disc sideways. At 0 it sits dead centre in the old rect;
    at an offset it straddles the edge, which is what a partially-successful
    drag leaves behind.
    """
    img = canvas()
    cx, cy = 1511 + offset, 574
    ys, xs = np.mgrid[0 : SCREEN[1], 0 : SCREEN[0]]
    r = np.sqrt((xs - cx) ** 2 + (ys - cy) ** 2)
    img[r <= ORB_PAINTED_RADIUS_PX] = ORB_IDLE_CORE
    img[(r > ORB_PAINTED_RADIUS_PX * 0.86) & (r <= ORB_PAINTED_RADIUS_PX)] = ORB_IDLE_RIM
    return img


def decoy_image():
    """Big bright UI blocks in the halo's exact colour, inside the old rect.

    The first version placed these at x=200..800 — outside the rect the analyser
    looks at. It reported zero bright pixels and passed, proving nothing. The
    decoys have to be where the analyser is actually looking.
    """
    img = canvas()
    img[450:500, 1380:1640] = HALO_COLOUR       # a "title bar"
    img[520:660, 1400:1620] = HALO_EDGE_COLOUR  # a "content panel"
    img[680:710, 1380:1640] = HALO_COLOUR       # a "status bar"
    return img


def run_analyser(before, after, ox, oy, side, keep_dir=None, tag=""):
    """Runs the real analyser and returns (returncode, output)."""
    with tempfile.TemporaryDirectory() as tmp:
        b = os.path.join(tmp, "before.png")
        a = os.path.join(tmp, "after.png")
        Image.fromarray(before).save(b)
        Image.fromarray(after).save(a)
        if keep_dir:
            os.makedirs(keep_dir, exist_ok=True)
            Image.fromarray(before).save(os.path.join(keep_dir, f"{tag}-before.png"))
            Image.fromarray(after).save(os.path.join(keep_dir, f"{tag}-after.png"))
        proc = subprocess.run(
            [sys.executable, ANALYSER, b, a, str(ox), str(oy), str(side)],
            capture_output=True,
            text=True,
        )
    return proc.returncode, proc.stdout + proc.stderr


def verdict_of(output):
    for line in output.splitlines():
        if line.startswith("VERDICT:"):
            return line.split(":", 1)[1].strip()
    return "NO-VERDICT-LINE"


def bright_count(output):
    """How many new bright pixels the analyser actually saw in the old rect.

    A negative control that never puts a single bright pixel where the analyser
    looks is not a control — it passes no matter what the analyser does. Two of
    the five controls here were exactly that until they were fixed, so every
    negative now has to show its work.
    """
    for line in output.splitlines():
        if "new bright pixels inside the OLD rect:" in line:
            return int(line.split(":")[-1].strip())
    return -1


def fitted_radius(output):
    for line in output.splitlines():
        if "circle fit:" in line:
            for token in line.split():
                if token.startswith("radius="):
                    return float(token[len("radius=") :].rstrip("px"))
    return None


def report(name, ok, detail):
    mark = "ok  " if ok else "FAIL"
    print(f"[{mark}] {name}")
    for line in detail.strip().splitlines():
        print(f"        {line}")


def check_negative(name, before, after, ox, oy, side, keep, tag, failures):
    """A negative control: must not be REPRODUCED, and must not be vacuous."""
    code, out = run_analyser(before, after, ox, oy, side, keep, tag)
    v = verdict_of(out)
    n = bright_count(out)
    if n <= 0:
        report(
            name,
            False,
            f"VACUOUS CONTROL: it put {n} bright pixels where the analyser looks, "
            f"so it passes whatever the analyser does\n{out}",
        )
        return failures + 1
    ok = v != "REPRODUCED"
    report(name, ok, f"verdict={v} bright_pixels_seen={n}\n{out}")
    return failures + (0 if ok else 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", default="", help="also write the synthetic pairs here")
    args = ap.parse_args()

    if not os.path.exists(ANALYSER):
        print(f"analyser not found: {ANALYSER}")
        return 1

    # The old rect the orb owned, centred on the orb: 298 px square at the
    # centre used in the report (1511,574 -> left 1362, top 425).
    ox, oy, side = 1362, 425, ORB_SIDE_PX
    base = canvas()
    failures = 0

    # 1. POSITIVE -----------------------------------------------------------
    code, out = run_analyser(base, arc_image(), ox, oy, side, args.keep, "arc")
    v = verdict_of(out)
    r = fitted_radius(out)
    ok = v == "REPRODUCED" and r is not None and abs(r - HALO_RADIUS_PX) <= 8.0
    failures += 0 if ok else 1
    report(
        "POSITIVE  a known halo must be found, at the drawn radius",
        ok,
        f"verdict={v} fitted_radius={r} drawn={HALO_RADIUS_PX}\n{out}",
    )

    # 2. NEGATIVE -----------------------------------------------------------
    code, out = run_analyser(base, base.copy(), ox, oy, side, args.keep, "empty")
    v = verdict_of(out)
    ok = v == "CLEAN"
    failures += 0 if ok else 1
    report("NEGATIVE  an unchanged screen must be clean", ok, f"verdict={v}\n{out}")

    # 3. DISC ---------------------------------------------------------------
    failures = check_negative(
        "NEGATIVE  the orb's own body is not a halo (the failed-drag case)",
        base,
        disc_image(),
        ox,
        oy,
        side,
        args.keep,
        "disc",
        failures,
    )

    # 3b. DISC-EDGE ---------------------------------------------------------
    failures = check_negative(
        "NEGATIVE  a half-vacated orb at the rect edge is not a halo either",
        base,
        disc_image(offset=110),
        ox,
        oy,
        side,
        args.keep,
        "disc-edge",
        failures,
    )

    # 4. DECOYS -------------------------------------------------------------
    failures = check_negative(
        "NEGATIVE  bright UI in the halo's own colour is not a halo",
        base,
        decoy_image(),
        ox,
        oy,
        side,
        args.keep,
        "decoy",
        failures,
    )

    print()
    if failures:
        print(f"SELFTEST FAILED: {failures} control(s) disagree with the analyser.")
        print("Any hunt result from this analyser must be reported as INCONCLUSIVE.")
        return 1
    print("SELFTEST PASSED: the analyser fires on a known halo and stays quiet on")
    print("a disc, on an empty screen, and on decoy application pixels.")
    return 0


if __name__ == "__main__":
    sys.exit(main())