# egui-notify issue package

Target: `ItsEthra/egui-notify` — issue-first strategy to align on API design
before the PR. **✅ Opened as [#54](https://github.com/ItsEthra/egui-notify/issues/54)**
on 2026-09-21 via `gh` (account `mahdimoslemi88-sys`); the draft below is what
was submitted (body compressed from this draft to keep it terse). No existing
issue covered this beforehand (searched "width" / "limit").

| File in this folder | Purpose |
|---|---|
| `ISSUE.md` | This document — copy-paste title + body below |
| `toast-max-width.patch` | Prototype patch implementing optional capping (open on demand) |
| `examples/toast_width_cap.rs` | Demo example for the prototype |
| `PR.md` | The PR text, ready once the design is agreed |

Strategy: post the issue first; link the prototype patch in a follow-up
comment only if the maintainer asks for code, or attach it upfront — either
works. After maintainer feedback, adjust the PR patch to the agreed API.

---

## Title

```
No way to cap toast width — long unbreakable captions stretch the card arbitrarily
```

## Body

```markdown
### Summary

Toast width is re-measured from the rendered content every frame (nice!),
but there is no upper bound. A long caption without word boundaries — a
URL, a file path, minified JSON, a hash — stretches the card to at least
the full text width, because there is no word boundary to wrap at.

In practice that means one long token can push a toast across a good part
of the screen, which is rarely what an app wants for notifications.

### Current knobs and why they don't help

- `Toast::width(w)` only seeds the **first frame** (animation start and
  pre-measure spacing); the card is re-measured from content on the next
  frame, so it is not a cap and its doc currently overstates what it does.
- `Toasts::with_padding` only affects spacing inside the card.
- Wrapping is `TextWrapMode::Extend` in `Toast::show`, with no cap involved.

### Proposal

An optional **width cap** at two levels, with zero behavior change when
unset:

- `Toasts::with_max_width(f32)` — channel-wide default.
- `Toast::max_width(impl Into<Option<f32>>)` — per-toast override that wins
  over the channel cap (pass `None` to opt a single toast out).

When capped and the caption exceeds the remaining width (cap − padding −
icon/cross slots), lay the caption out with `TextWrapMode::Wrap` at that
target width, so the card grows vertically instead of stretching
horizontally. Everything stays re-measured from the actual galley per
frame, so sub-cap toasts keep the current snug auto-width behavior.

### Design questions where maintainer input matters

1. **Naming**: `max_width`? `wrap_width`? `max_card_width`?
2. **Levels**: is a per-toast override worth it, or is a channel-level cap
   enough? (I have a use case for per-toast: dictionary previews are short,
   engine transcripts can be long.)
3. **Default**: should `Toasts::default()` ship with a sane cap (e.g.
   something screen-relative like `ctx.screen_rect().width() * 0.6`)? I lean
   towards opt-in (`None` default) to stay conservative, but a screen-
   relative default would arguably fix the footgun for everyone.
4. **Exact wrap-target formula**: cap minus what exactly — padding only, or
   also the icon and close-button slots?

### Repro sketch

A toast with the caption
`"https://example.com/a/very/long/path/segment/that/has/no/spaces/at/all?query=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"`
on any anchor — the card stretches to the full text width.

Happy to send a PR with the opt-in variant; a working prototype against
current `main` is ready (about +60 lines across `lib.rs`/`toast.rs`,
including doc fixes for `Toast::width`).
```

---

## Checklist ارسال (فارسی)

1. **باز کردن issue**: <https://github.com/ItsEthra/egui-notify/issues/new> — عنوان و بدنهٔ بالا کپی شود (بدنه انگلیسی است چون زبان رسمی بالادست است).
2. **پس از باز شدن**: شمارهٔ issue را در `PR.md` بخش `Fixes` اضافه کن (مثلاً `Fixes #NN`) تا PR به‌طور خودکار به issue لینک شود.
3. **اگر maintainer پرسید دربارهٔ prototype**: پچ `toast-max-width.patch` را به‌عنوان کامنت (attachment) یا لینک شاخهٔ `feature/toast-max-width` فورک‌شده بفرست.
4. **پس از توافق روی API**: اگر نام یا فرمول تغییر کرد، پچ را مطابق بازخورد اصلاح کن و بعد PR را باز کن — بدنهٔ PR در `PR.md` بخش «Design questions» را با پاسخ‌های maintainer هم‌گام کن.

نکته: گام ۲–۴ نیازمند حساب گیت‌هاب با دسترسی push از همین دستگاه است (SSH یا PAT).
