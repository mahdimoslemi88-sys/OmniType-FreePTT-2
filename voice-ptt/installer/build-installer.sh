#!/usr/bin/env bash
# -----------------------------------------------------------------------
# Build the OmniType FreePTT installer.
#
#   installer/build-installer.sh
#
# Reads the version out of Cargo.toml, checks it really looks like a version,
# and only then hands it to Inno Setup.
#
# The check is the point. The first two attempts parsed Cargo.toml inside the
# .iss (and then inside batch), and produced installers named
# "51061184-setup.exe" and "55643536-setup.exe" **without any error** — the .exe
# inside was still correct, so nothing else in the build complained. The only
# symptom was a filename that matches no release and that the updater can never
# match a tag against.
# -----------------------------------------------------------------------
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO="$ROOT/Cargo.toml"
ISS="$ROOT/installer/omnitype.iss"

if [[ ! -f "$CARGO" ]]; then
  echo "[build-installer] Cargo.toml not found at $CARGO" >&2
  exit 1
fi

# The first `version = "..."` line in this file is the [package] table's; a
# dependency version further down cannot be reached first.
VERSION="$(grep -m1 '^version' "$CARGO" | sed -e 's/.*=[[:space:]]*"//' -e 's/".*//' -e 's/\r//')"

if [[ ! "$VERSION" =~ ^[0-9]+(\.[0-9]+)*$ ]]; then
  echo "[build-installer] refusing to build: '$VERSION' is not a version" >&2
  exit 1
fi

echo "[build-installer] version from Cargo.toml: $VERSION"

ISCC=""
for candidate in \
  "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" \
  "/c/Program Files/Inno Setup 6/ISCC.exe"; do
  if [[ -x "$candidate" ]]; then ISCC="$candidate"; break; fi
done

if [[ -z "$ISCC" ]] && command -v ISCC.exe >/dev/null 2>&1; then
  ISCC="$(command -v ISCC.exe)"
fi

if [[ -z "$ISCC" ]]; then
  echo "[build-installer] Inno Setup 6 not found. Install from https://jrsoftware.org/isdl.php" >&2
  exit 1
fi

echo "[build-installer] compiler: $ISCC"
cd "$ROOT"
"$ISCC" "$ISS" "-DAppVersion=$VERSION"

OUT="$ROOT/installer/Output/OmniType-FreePTT-$VERSION-setup.exe"
if [[ ! -f "$OUT" ]]; then
  echo "[build-installer] expected $OUT but it does not exist" >&2
  exit 1
fi

echo "[build-installer] done: $OUT"
