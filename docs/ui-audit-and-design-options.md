# UI Audit & Design Options — OmniType FreePTT v2

> تاریخ: ۲۰۲۶-۰۹-۲۱ · پایه: `egui 0.28 / eframe 0.28` · مهارت: `omnitype-rust-ui`
> وضعیت: **در انتظار تأیید کاربر** (Preview-First Gate)

---

## بخش ۱: ممیزی وضعیت موجود

### ✅ موارد compliant با اسکیل

| قانون | وضعیت | شواهد |
|---|---|---|
| پالت متمرکز | ✅ | تمام ۱۲۲ مورد `Color32::from_rgb` فقط داخل `mod palette` (overlay.rs 76-278) |
| هلپرهای کروم | ✅ | `manager_central_panel`، `manager_card`، `status_chip`، `capsule_frame`، `header_badge` |
| دوگانگی تم | ✅ | `dark` (پیش‌فرض) + `light` (`#[cfg(feature = "light-theme")]`) |
| Persian RTL + ZWNJ | ✅ | `format_persian_display` (reshaper + bidi)، نیم‌فاصله در لیبل‌ها |
| Single-instance guard | ✅ | `Local\OmniTypeFreePTT.SingleInstance` در `lib.rs` |
| Hide vs Full Exit | ✅ | فلگ‌های اتمیک `overlay_flag` / `quit_flag` + `HotkeyEvent::Quit` |
| فونت‌های فارسی | ✅ | `segoe_ui` + `tahoma` + `seguisym` + phosphor در `lib.rs` |
| ۴ حالت بصری کپسول | ✅ | `IdleDormant` / `HoveredAwake` / `RecordingActive` / `Processing` |
| کامپایل تمیز | ✅ | `cargo check --all-targets` بدون خطا |

### 🔴 نقص‌های بحرانی (نقض قانون اسکیل)

#### نقص ۱ — In-App Management Defect (بخش ۳ اسکیل)

> [!WARNING]
> اسکیل: «Selecting settings or dictionary management must **never** launch external IDEs or text editors like VS Code.»

**۴ محل نقض پیدا شد:**

| # | فایل | خط | محتوا |
|---|---|---|---|
| A | `gui/tray.rs` | 60، 133-140 | منوی `Open config.toml` → `cmd /C start` |
| B | `gui/tray.rs` | 62، 141-148 | منوی `Open dictionary.toml` → `cmd /C start` |
| C | `gui/overlay.rs` | ~1120 | دکمه `ویرایش در Notepad` در Dictionary Manager |
| D | `gui/overlay.rs` | 1554 | دکمه `باز کردن config.toml` در Engine Manager |

#### نقص ۲ — Settings Window وجود ندارد

اسکیل بخش ۳ یک پنجره تنظیمات درون‌برنامه‌ای **الزامی** می‌داند. فعلاً `show_settings_window`، `render_settings_window` و فلگ `settings_flag` **اصلاً وجود ندارند**. کاربر برای تغییر VAD/hotkey/engine مجبور است فایل خام را در Notepad باز کند (نقص ۱).

#### نقص ۳ — ترَی منو با بخش ۸ اسکیل همخوان نیست

| اسکیل می‌خواهد | فعلی |
|---|---|
| Open OmniType | `Show / Hide overlay` ✅ (معادل) |
| Active Engine | `AI Models & Engines` ✅ |
| Dictionary | `Dictionary Manager` ✅ |
| **Settings** | ❌ وجود ندارد — به جای آن `Open config.toml` |
| Exit completely | `Quit` ✅ (اما برچسب فارسی/صریح نیست) |

---

## بخش ۲: گزینه‌های طراحی

### گزینه الف — Minimal Compliance (حداقل رفع نقص) ⭐ سبک

**دامنه:** فقط رفع نقص ۱ + ۳. Settings Window ساخته نمی‌شود.

```
┌─ تغییرات ─────────────────────────────────────┐
│ tray.rs:  حذف ۲ منوی فایل خام                  │
│           افزودن منوی "Settings (تنظیمات)"     │
│           → sets settings_flag                 │
│ overlay.rs: حذف دکمه Notepad (نقص C)           │
│             حذف دکمه config.toml (نقص D)       │
│ lib.rs:    سیم‌کشی settings_flag → OverlayApp  │
└────────────────────────────────────────────────┘
```

- **مزیت:** کمترین ریسک، سریع‌ترین، بدون UI جدید
- **عیب:** کاربر همچنان برای تنظیمات به فایل خام وابسته است → اسکیل بخش ۳ ناقص می‌ماند

---

### گزینه ب — Full In-App Settings Window ⭐⭐ توصیه‌شده

**دامنه:** گزینه الف + ساخت Settings Window کامل (نقص ۲).

```
┌─ پنجره: «تنظیمات — OmniType» 560×580 ──────────────────────┐
│                                                            │
│  ⚙ تنظیمات                          [dark theme badge]     │
│  پیکربندی صوتی، موتور تشخیص گفتار و کلیدهای میانبر          │
│  ─────────────────────────────────────────────────────────  │
│  ┌─ 🎙 صوت ──────────────────────────────────────────────┐  │
│  │ دستگاه ورودی:  [default        ▾]                     │  │
│  │ نرخ نمونه‌برداری: [16000 Hz ▾]  کانال: [مونو ▾]        │  │
│  │ تقویت نرم‌افزاری: [────●────] 0.0 dB                   │  │
│  └───────────────────────────────────────────────────────┘  │
│  ┌─ 🧠 موتور تشخیص گفتار ────────────────────────────────┐  │
│  │ موتور فعال: ( ) auto  ( ) google  ( ) local_whisper   │  │
│  │            ( ) groq/cloud                             │  │
│  │ مدل محلی: [base ▾]    زبان: [fa ▾]                    │  │
│  │ Cloud daily limit: [300]    timeout: [30s]            │  │
│  └───────────────────────────────────────────────────────┘  │
│  ┌─ 🤚 تشخیص سکوت (VAD) ─────────────────────────────────┐  │
│  │ حساسیت: [────●────] 0.50                               │  │
│  │ مدت سکوت برای توقف: [1500 ms]                          │  │
│  │ حداقل مدت گفتار: [150 ms]                              │  │
│  │ ☐ توقف ضبط با سکوت حتی با نگه‌داشتن کلید               │  │
│  └───────────────────────────────────────────────────────┘  │
│  ┌─ ⌨ کلیدهای میانبر ────────────────────────────────────┐  │
│  │ ضبط: [CapsLock]        نمایش/مخفی: [Ctrl+Alt+S]        │  │
│  │ خروج: [Ctrl+Alt+Q]                                     │  │
│  └───────────────────────────────────────────────────────┘  │
│  ┌─ 🎨 رابط کاربری ──────────────────────────────────────┐  │
│  │ ☑ نمایش کپسول شناور                                   │  │
│  │ تم: ( ) تیره  ( ) روشن                                 │  │
│  └───────────────────────────────────────────────────────┘  │
│                                                            │
│  [ذخیره تنظیمات]  [بارگذاری مجدد از فایل]   config.toml ✓   │
└────────────────────────────────────────────────────────────┘
```

**نکات طراحی:**
- بازاستفاده از `manager_central_panel` + `manager_card` + `manager_header` (هیچ کروم جدیدی)
- تمام رنگ‌ها از `palette::` (قرار دادن `TEXT_LABEL`، `ACCENT`، `CARD_BG_ALT` و ...)
- اعتبارسنجی inline: دکمه ذخیره در صورت نامعتبر بودن غیرفعال می‌شود (حالت ۹ اسکیل)
- نوشتن اتمیک روی `config_path` با `Settings::save` (همان منطق موجود)
- چیدمان RTL با `format_persian_display` برای همه لیبل‌ها
- **بدون** باز کردن هرگونه ویرایشگر خارجی

- **مزیت:** تطابق کامل با بخش ۳ اسکیل، رفع هر ۳ نقص
- **عیب:** ~۲۲۰ خط کد جدید، نیاز به سیم‌کشی فلگ جدید از `tray.rs` → `lib.rs` → `OverlayApp`

---

### گزینه ج — Full + Engine/Dict Hardening ⭐⭐⭐ کامل

**دامنه:** گزینه ب + تقویت پنجره‌های موجود:

1. **Dictionary Manager:** افزودن ستون ویرایش inline (حالت ۱۰ اسکیل: «inline editing fields») — فعلاً فقط حذف داریم.
2. **Engine Manager:** ماسک کردن کلید API (`gsk_...3a1f`) در نمایش (بخش ۳ اسکیل: «safely masked»).
3. **First-Run State:** نشانگر «No Model Loaded» در کپسول (حالت ۱ اسکیل).
4. **Cloud Consent:** درخواست صریح opt-in قبل از ارسال صوت (حالت ۵ اسکیل).

- **مزیت:** پوشش کامل ۱۰ حالت اسکیل
- **عیب:** بزرگ‌ترین تغییر، بالاترین ریسک regression

---

## بخش ۳: توصیه

**توصیه:** گزینه **ب** — رفع هر ۳ نقص بحرانی با یک UI یکپارچه، بدون بازطراحی کپسول اصلی (که پایدار است).
گزینه ج را می‌توان به صورت فاز دوم و مستقل اجرا کرد.

---

## بخش ۵: وضعیت پیاده‌سازی (گزینه ج — اجرا شد)

کاربر گزینه **ج** را انتخاب کرد. تمام موارد پیاده‌سازی و اعتبارسنجی شدند:

| # | مورد | وضعیت |
|---|------|--------|
| 1 | ترَی منو مطابق بخش ۸ اسکیل (برچسب‌های دوزبانه + حذف منوهای ویرایشگر خارجی) | ✅ انجام شد |
| 2 | پنجره تنظیمات درون‌برنامه‌ای (`render_settings_window`) — کارت‌های Audio / ASR / VAD / Hotkeys | ✅ انجام شد |
| 3 | پرچم `settings_flag` (tray → `lib.rs` → `OverlayApp`) | ✅ انجام شد |
| 4 | حذف هر ۴ محل اجرای ویرایشگر خارجی (Notepad / `cmd /C start`) | ✅ انجام شد |
| 5 | ویرایش inline دیکشنری (ستون ویرایش + تأیید/لغو) | ✅ انجام شد |
| 6 | ماسک کردن کلید API (`mask_secret`) + خط وضعیت ماسک‌شده | ✅ انجام شد |
| 7 | نشانگر «مدلی بارگذاری نشده» در کپسول (first-run state) | ✅ انجام شد |
| 8 | پنجره رضایت آگاهانه برای موتور ابری (`render_consent_window`) | ✅ انجام شد |
| 9 | `validate_settings` با پیام‌های خطای فارسی | ✅ انجام شد |
| 10 | حذف `TrayCommand` مرده | ✅ انجام شد |

### نتایج اعتبارسنجی (بخش ۷ اسکیل)

| دستور | نتیجه |
|--------|--------|
| `cargo check --all-targets` | ✅ بدون خطا |
| `cargo check --all-targets --features light-theme` | ✅ بدون خطا |
| `cargo clippy --all-targets` | ✅ صفر warning |
| `cargo test --lib` | ✅ ۹۸ passed / ۰ failed |

### فایل‌های تغییر یافته

- `voice-ptt/src/gui/tray.rs` — بازنویسی منو + حذف `TrayCommand`
- `voice-ptt/src/gui/mod.rs` — به‌روزرسانی re-export
- `voice-ptt/src/lib.rs` — سیم‌کشی `settings_flag` + مسیر دیکشنری
- `voice-ptt/src/gui/overlay.rs` — پنجره‌های تنظیمات/رضایت، ویرایش inline، ماسک کلید، first-run

## بخش ۴: قوانین اجرایی

- هیچ کد قدیمی کامنت نمی‌شود — حذف یا جایگزینی مستقیم.
- هیچ `Color32::from_rgb` خارج از `mod palette` اضافه نمی‌شود.
- فقط از هلپرهای کروم موجود استفاده می‌شود.
- اعتبارسنجی: `cargo check --all-targets` + `--features light-theme` + `cargo clippy --all-targets` + `cargo test --lib`.
