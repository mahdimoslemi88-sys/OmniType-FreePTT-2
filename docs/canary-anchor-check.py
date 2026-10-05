#!/usr/bin/env python
"""Fail if a canary script's anchors no longer match the code they mutate.

The harness prints SKIP when an anchor is missing and carries on. That is the
right behaviour *during* a run — a partial result beats none — and the wrong
behaviour as the last word: a stage whose anchors rotted looks exactly like a
stage whose bugs were all caught, and the two are told apart only by reading the
output closely.

So this is the check that runs *before* believing a canary result:

    python ../docs/canary-anchor-check.py mutation-check-session.sh \\
        mutation-check-coordinator.sh

Run from v-2/voice-ptt. Exits non-zero, naming the mutation, if any anchor is
absent from its file. It does not judge whether the mutation is a *good* one —
only whether it would run at all. Script names are looked up next to this file,
so the call above works from the crate directory; code paths are resolved
against the current directory, which is the crate root during that call.
"""
import io
import os
import re
import sys

# `mutate "$FILE" 'FROM' 'TO' 'NAME'`, after bash's line continuations have been
# taken out — the harness is a shell script, so the real text it passes to argv
# is the joined version, and this must join it the same way. Single-quoted
# literals are what this parser looks for on purpose: the harness passes them
# straight to argv, so anything else would be a lie.
CONTINUATION = re.compile(r"\\\r?\n")
MUTATE = re.compile(
    r"""^mutate\s+"\$(\w+)"\s+'(.*?)'\s+'(.*?)'\s+'(.*?)'\s*$""",
    re.MULTILINE | re.DOTALL,
)

VARS = {}
HERE = os.path.dirname(os.path.abspath(__file__))


def load(path):
    return io.open(path, encoding='utf-8', newline='').read()


def main(scripts):
    root = '.'
    missing = 0
    total = 0
    for script in scripts:
        text = CONTINUATION.sub('', load(os.path.join(HERE, script)))
        # `V=src/...` assignments, so a script can be checked without running it.
        for name, target in re.findall(r"^(\w+)=(\S+)\s*$", text, re.MULTILINE):
            VARS[name] = target
        for var, frm, _to, name in MUTATE.findall(text):
            target = VARS.get(var)
            if target is None:
                print(f"  ?? {name}: unknown file variable ${var}")
                missing += 1
                continue
            total += 1
            body = load(root + '/' + target)
            if frm not in body:
                missing += 1
                print(f"  SKIP {name}\n       anchor not in {target}:")
                print('       ' + frm.strip().splitlines()[0][:70])
    print(f"{total - missing}/{total} anchors match")
    return 1 if missing else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))