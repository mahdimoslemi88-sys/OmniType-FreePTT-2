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
#
# 5. One run at a time. Two runs share one snapshot directory: the older one
#    restores from (and deletes) files the newer one is still using, so
#    mutations survive the run and the *next* run snapshots them as its new
#    baseline. That is not hypothetical — it left four mutated files and 22
#    failing tests behind, and it reported "CAUGHT" for canaries whose verdict
#    came from the other run's leftovers.

CANARY_RED='\x1b[31m'
CANARY_LOCK=".canary-lock"

canary_unlock() {
  rm -f "$CANARY_LOCK"
}

# Refuses to start while another run is alive. A lock file whose pid is gone is
# stale and gets taken over: the common case is an interrupted run, and refusing
# on that alone would make the harness unusable after any Ctrl-C.
canary_lock() {
  if [ -f "$CANARY_LOCK" ]; then
    local old
    old=$(cat "$CANARY_LOCK" 2>/dev/null || echo "")
    if [ -n "$old" ] && kill -0 "$old" 2>/dev/null; then
      echo "ABORT: a canary run is already alive (pid $old)."
      echo "       Two runs share one snapshot directory and corrupt each other."
      echo "       Kill pid $old, or delete $CANARY_LOCK if you are sure it is dead."
      exit 1
    fi
    echo "note: taking over $CANARY_LOCK from dead pid ${old:-?}"
  fi
  echo $$ > "$CANARY_LOCK"
}

canary_init() {
  CANARY_FILES=("$@")
  CANARY_SNAP=".canary-snapshot"

  # Rule 5, before anything is created or deleted.
  canary_lock

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
    cp "$f" "$CANARY_SNAP/$i" || {
      echo "ABORT: could not snapshot $f — a run without a full snapshot restores nothing"
      exit 1
    }
    i=$((i + 1))
  done
  trap 'canary_restore_all; canary_unlock' EXIT
}

# Restores every snapshotted file from the pristine copy.
canary_restore_all() {
  # `canary_finish` restores, proves and deletes the snapshot; after that the
  # EXIT trap has nothing left to do and must not cry wolf about it.
  if [ "${CANARY_RESTORE_DONE:-0}" = "1" ]; then
    return 0
  fi
  # A missing snapshot means this run never took one (or another run deleted
  # it). Restoring from a directory that is not there would leave every
  # mutation in place, so say so loudly instead.
  if [ ! -d "$CANARY_SNAP" ]; then
    echo "!! $CANARY_SNAP is gone; files were NOT restored by the harness" >&2
    return 1
  fi
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
  CANARY_RESTORE_DONE=1
  cargo test --lib 2>&1 | grep -E 'test result'
  if [ -d .canary-snapshot ]; then
    rm -rf .canary-snapshot
    echo "snapshot cleaned"
  fi
  canary_unlock
}