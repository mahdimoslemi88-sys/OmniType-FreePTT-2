#!/usr/bin/env bash
# Shared mutation-canary engine.
#
# Why this lives in its own file: two copies of a *judge* that decides whether
# the tests are adequate is exactly how one of them quietly goes stale. Every
# bug listed below was found by this harness being the thing that was wrong,
# not by the code under test.
#
# Caller contract:
#   canary_init src/a.rs src/b.rs ...   # snapshot + refuse to start on a red tree
#   mutate FILE FROM TO "name"           # one decision, mutated and tested
#   canary_finish                        # restore, then prove the tree is green
#
# What this guarantees, each one bought by a bug that got through:
#
# 1. It refuses to start on a red tree. A canary run against a tree that is
#    already failing measures nothing: every "CAUGHT" afterwards would be that
#    pre-existing failure, and the canary would look like it works.
#
# 2. It restores from ONE snapshot taken before the first mutation, never from
#    a rolling `.bak`. A rolling `.bak` is a stack — an interrupted run leaves a
#    mutated file *and* a `.bak`, and the next run snapshots the mutation as its
#    new baseline and restores it forever. That is not hypothetical: an
#    interrupted run's C4 and C14 mutations survived a complete later run that
#    reported both as CAUGHT, and the tree stayed red afterwards.
#
# 3. `trap ... EXIT` puts the files back even when the run is killed. The trap is
#    the only thing standing between a Ctrl-C and a poisoned tree.
#
# 4. `NOBUILD` is reported separately from `MISSED`. A mutation that does not
#    compile runs zero tests, so "no test noticed" and "nothing ran" produce the
#    same empty output — and the naive verdict calls its own blindness a result.

CANARY_RED='\x1b[31m'

canary_init() {
  CANARY_FILES=("$@")
  CANARY_SNAP=".canary-snapshot"
  rm -rf "$CANARY_SNAP"
  mkdir -p "$CANARY_SNAP"

  # Rule 1. A red starting tree invalidates every result that follows.
  if ! cargo test --lib >/tmp/canary-preflight.log 2>&1; then
    echo "ABORT: the tree is already red. Fix that first; a canary run from here"
    echo "       measures the pre-existing failure, not the mutation."
    grep -E '^test .*FAILED|^test result' /tmp/canary-preflight.log | head -20
    exit 1
  fi

  # Rule 2: one pristine copy per file, before anything is touched.
  local i=0
  for f in "${CANARY_FILES[@]}"; do
    cp "$f" "$CANARY_SNAP/$i"
    i=$((i + 1))
  done
  trap canary_restore_all EXIT
}

# Restores every snapshotted file from the pristine copy.
canary_restore_all() {
  local i=0
  for f in "${CANARY_FILES[@]}"; do
    if [ -f "$CANARY_SNAP/$i" ]; then
      cp "$CANARY_SNAP/$i" "$f"
      touch "$f"
    fi
    i=$((i + 1))
  done
}

canary_restore_one() {
  local want="$1" i=0
  for f in "${CANARY_FILES[@]}"; do
    if [ "$f" = "$want" ]; then
      cp "$CANARY_SNAP/$i" "$f"
      touch "$f" # newer than the binary the mutation built
      return 0
    fi
    i=$((i + 1))
  done
  echo "ABORT: $want is not in the snapshot list"
  return 1
}

mutate() {
  local file="$1" from="$2" to="$3" name="$4"
  python - "$file" "$from" "$to" <<'PY'
import io, sys
path, frm, to = sys.argv[1], sys.argv[2], sys.argv[3]
s = io.open(path, encoding='utf-8').read()
if frm not in s:
    sys.exit("ANCHOR NOT FOUND in " + path + ": " + frm[:70])
io.open(path, 'w', encoding='utf-8').write(s.replace(frm, to, 1))
PY
  if [ $? -ne 0 ]; then echo "SKIP    $name (anchor missing)"; return; fi

  local out
  out=$(cargo test --lib 2>&1)
  local failed
  failed=$(printf '%s' "$out" | grep -oE '^    [a-z_]+::[a-z_:]+' | sed 's/^    //' | sort -u)
  local total
  total=$(printf '%s' "$out" | grep -oE '[0-9]+ passed' | head -1)
  local build
  build=$(printf '%s' "$out" | grep -cE '^error\[E[0-9]+\]|^error: could not compile')

  canary_restore_one "$file"

  if [ "$build" -gt 0 ]; then
    printf '%sNOBUILD %-51s mutation did not compile — verdict unknown\n' "$CANARY_RED" "$name"
  elif [ -n "$failed" ]; then
    printf 'CAUGHT  %-52s %s\n' "$name" "$total"
    printf '        %s\n' $failed
  else
    printf '%sMISSED  %-52s suite still green\n' "$CANARY_RED" "$name"
  fi
}

# Restores, then proves it. Rule 4 of lesson 14: a restore is a claim, and the
# claim has to be measured.
canary_finish() {
  echo
  echo "=== restoring and confirming the tree is green again ==="
  canary_restore_all
  cargo test --lib 2>&1 | grep -E 'test result'
  if [ -d .canary-snapshot ]; then
    rm -rf .canary-snapshot
    echo "snapshot cleaned"
  fi
}