# egui-notify PR package

Target repo: `ItsEthra/egui-notify` — branch `main` @ `18ac220` (v0.23.0, egui 0.36).
Local clone: `C:\Users\<you>\AppData\Local\Temp\egui-notify` (branch `feature/toast-max-width`).

## Contents of this folder

| File | What it is |
|---|---|
| `toast-max-width.patch` | `git diff` of the two-file change (lib.rs, toast.rs) — applies cleanly on pristine `main` (verified with `git apply --check`) |
| `examples/toast_width_cap.rs` | Demo example; compiles against the patched crate (`cargo check --example toast_width_cap` ✅) |
| `PR.md` | This document |

---

## Title

```
Add optional toast width cap: `Toasts::with_max_width` / `Toast::max_width`
```

## Body

```markdown
## Summary

Since the 0.19 width rework, a toast's width is fully automatic — it is
re-measured from the rendered content every frame. That is great UX, but
there is no way to cap it: a long single-word caption (URL, file path,
minified JSON…) stretches the card to the full text width, because there is
no word boundary to wrap at. The doc on `Toast::width` ("set the exact
width") is also stale — it only seeds the first frame.

This PR adds an **optional width cap** at two levels, with zero behavior
change when unset:

- **`Toasts::with_max_width(f32)`** — channel-wide default, applied to every
  toast on the channel.
- **`Toast::max_width(impl Into<Option<f32>>)`** — per-toast override that
  beats the channel cap (pass `None` to opt a single toast out).

### How it works

In `Toast::show`, the caption galley is laid out as today (unwrapped). If a
cap is set and the unwrapped caption exceeds the remaining width (card cap
minus padding, minus one `padding.x` for the icon/cross slots), the caption
is re-laid out with `TextWrapMode::Wrap` at that target width. Width is
still re-measured from the actual galley every frame, so:

- The card is never wider than the cap, and no wider than its content when
  under the cap — auto-width behavior is preserved, only bounded.
- Unset cap ⇒ byte-for-byte the old code path (probe galley **is** the
  caption galley, no re-layout, no extra allocation).
- Caption row heights drive icon/cross sizing exactly as before, so
  icon size and vertical metrics are unchanged.

### Behavior matrix

| Channel cap | Toast cap | Result |
|---|---|---|
| — | — | fully automatic (unchanged default) |
| ✓ | — | wraps at channel cap |
| ✓ | ✓ | wraps at toast cap (per-toast wins) |
| ✓ | `None` | fully automatic (per-toast opt-out) |
| — | ✓ | wraps at toast cap |

`max_width <= 0` is treated as unset, so existing `set_width(w).max_width(0.)`-style
workarounds keep working.

### Docs fixes included

- Rewrites `Toast::width` docs to describe what it actually does: seeds the
  first-frame width (animation start / pre-measure spacing), with the card
  re-measured every frame thereafter.
- New public methods are documented; `#[warn(missing_docs)]` stays clean.

### Testing

- `cargo check --all-targets` — clean
- `cargo clippy --all-targets` — no new warnings (the two warnings on current
  `main` are pre-existing: undocumented `with_default_font` and the unused
  `commit` key in the manifest)
- New example `toast_width_cap.rs` demonstrating both APIs (auto / capped /
  opt-out buttons, channel cap 260 px)

### Notes for maintainers

- English wording / API naming is negotiable (e.g. `wrap_width`,
  `max_card_width`), as is the exact wrap-target formula (currently
  `cap − padding.x·2 − icon/cross slots`); happy to adjust.
- `with_max_width` is `const` to match `with_padding`/`with_margin` style on
  the builder.

Fixes the "no way to limit toast width" gap left by the 0.19 auto-width
rework.
```

## Copy-paste commands

```bash
# 1. Fork https://github.com/ItsEthra/egui-notify on GitHub (web UI), then:

# 2. Use the ready branch in the local clone
cd /tmp/egui-notify   # %LOCALAPPDATA%\Temp\egui-notify
git checkout feature/toast-max-width
git remote add fork https://github.com/<USERNAME>/egui-notify.git
git push -u fork feature/toast-max-width

# 3. Open the PR
# https://github.com/ItsEthra/egui-notify/compare/main...<USERNAME>:egui-notify:feature/toast-copy
```

## Submission guide (فارسی)

### وضعیت بستهٔ PR

بستهٔ PR کامل است و همهٔ اعتبارسنجی‌ها را رد کرده:

| قلم | وضعیت |
|---|---|
| پچ دو فایلی (`lib.rs` + `toast.rs`) | ✅ روی `main` دست‌نخورده (`18ac220`) تمیز اعمال می‌شود |
| مثال دمو `toast_width_cap.rs` | ✅ مقابل crate پچ‌شده کامپایل می‌شود |
| هشدار جدید | ✅ صفر — دو هشدار `main` از قبل موجود است (doc متد `with_default_font` و کلید manifest) |
| شاخهٔ `feature/toast-max-width` | ✅ در کلون `/tmp/egui-notify` آمادهٔ push به فورک |

### گام‌های ارسال

1. **فورک کنید**: در گیت‌هاب، مخزن `ItsEthra/egui-notify` را فورک کنید.
2. **پوش کند**: از کلون آمادهٔ `/tmp/egui-notify` (شاخهٔ `feature/toast-max-width`) — دستورها در بخش «Copy-paste commands» بالاست. نام‌کاربری گیت‌هاب خود را جای `<USERNAME>` بگذارید.
3. **PR باز کنید**: لینک compare در همان بخش؛ عنوان و بدنه از `PR.md` کپی شود. بدنه به انگلیسی نوشته شده چون زبان رسمی بالادست است.
4. **جایگزین سریع بدون PR** (اگر منتظر مرج شدن نمی‌مانید): همان پچ را به‌عنوان `[patch.crates-io]` در `Cargo.toml` اصلی OmniType بیاورید (نیاز به پچ داخل `v-2` دارد)؛ این مسیر کوتاه‌مدت است و PR مسیر بلندمدت.

### نکتهٔ نهایی

اگر maintainer نام API یا فرمول عرض wrap را تغییر داد، هم در پچ و هم در متد `make_toast` پروژهٔ خودمان باید هم‌گام شود — پچ کوتاه است (۵۹ خط) و تطبیقش چند دقیقه کار دارد.

برای ارسال لازم است گیت‌هاب شما از همین دستگاه به `github.com` دسترسی push داشته باشد (SSH یا PAT). کلون `/tmp/egui-notify` و شاخهٔ آماده موقت‌اند — اگر سیستم ری‌استارت شود، از `toast-max-width.patch` در همین پوشه در یک کلون تازه می‌توان شاخه را بازسازی کرد:

```bash
git clone https://github.com/ItsEthra/egui-notify && cd egui-notify
git checkout -b feature/toast-max-width
git apply /path/to/toast-max-width.patch
git add -A && git commit -m "Add optional toast width cap"
```