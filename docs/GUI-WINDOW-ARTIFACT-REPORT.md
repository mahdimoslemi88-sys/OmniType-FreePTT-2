# گزارش مستقل: آرتیفکت پنجره در OmniType FreePTT (نوار روشن بالای Orb و کادر سفید دور باکس متن)

**تاریخ:** ۲۰۲۶-۰۹-۲۹ (۱۴۰۵-۰۷-۰۷) · **بیلد در حال اجرا:** `md5 = ff5ab23da5e07c7363e2c979893770c6` · **PID نمونه:** 7276
**هدف این سند:** یک گزارش کامل و مستقل از گفتگو، برای مشورت با یک مدل/مهندس خارجی. همهٔ اعداد، دستورها، مسیر فایلها و
فرضیهها در همین سند آمدهاند تا بدون دسترسی به تاریخچهٔ چت قابل استفاده باشد.

---

## ۱. خلاصهٔ یکپاراگرافی (برای کپی کردن در گفتگو با مدل خارجی)

> یک اپ دسکتاپ Rust با `eframe/egui 0.28.1` + بکاند `wgpu 0.20.1` (Vulkan) روی Windows 11 (build 26200, DPI 125%)
> داریم که یک «اورب» شنای شفاف روی دسکتاپ نشان میدهد. پنجرهٔ اصلی با
> `ViewportBuilder::with_decorations(false).with_transparent(true).with_always_on_top().with_resizable(false)` و
> `renderer: Renderer::Wgpu` و `clear_color = [0,0,0,0]` ساخته میشود. برای شفافیت پیکسلبهپیکسل، در زمان اجرا
> استایلهای فریم پنجره با `SetWindowLongW` حذف میشوند (`WS_CAPTION|WS_THICKFRAME|WS_SYSMENU|WS_MINIMIZEBOX|WS_MAXIMIZEBOX|WS_BORDER|WS_DLGFRAME`
> پاک و `WS_POPUP` ست میشود)، `WS_EX_WINDOWEDGE|WS_EX_CLIENTEDGE|WS_EX_STATICEDGE|WS_EX_DLGMODALFRAME` پاک میشوند و
> سپس `DwmSetWindowAttribute(DWMWA_NCRENDERING_POLICY=DISABLED, DWMWA_WINDOW_CORNER_PREFERENCE=DONOTROUND, DWMWA_BORDER_COLOR=NONE, DWMWA_SYSTEMBACKDROP_TYPE=NONE)`
> و `DwmExtendFrameIntoClientArea(-1,-1,-1,-1)` اعمال میشود.
>
> **مشکل فعلی:** یک **مستطیل روشن (سفید مایل به آبی)** درست بالای اورب ظاهر میشود و همان مستطیل دور «باکس متن»
> (پنجرهٔ جداگانهٔ رونویسی) هم دیده میشود. اندازهگیری پیکسلی روی اسکرینشات کاربر: نوارِ ۲۰۳×۲۸ پیکسل با رنگ
> `(224,241,255)` در لبهٔ چپ که به `(221,237,254)` در وسط میرسد (کانال آبی اشباع در ۲۵۴–۲۵۵)، و یک مستطیل سفید
> ۱۵٬۰۳۶ پیکسلی `(255,255,255)` دور کارت. لبههای مستطیل **تیز** و داخل آن **گرادیان ملایم** است.
> این آرتیفکت بعد از مجموعهای از اصلاحات ظاهر شد؛ یک بار علتش پیدا و حذف شد
> (`RedrawWindow(RDW_ERASE|RDW_FRAME)` که ناحیهٔ «فریم گستردهشده» را روشن میکرد) ولی **کاربر میگوید هنوز باقی است**.
> سؤال کلیدی: این پیکسلهای روشن مالِ کدام پنجرهاند — پنجرهٔ خودمان است که ناحیهٔ نقاشینشدهاش با لایهٔ روشن
> DWM/سیستم رنگ میشود، یا یک پنجرهٔ سومشخص، یا رفتار `alpha` در سواپچین wgpu؟

---

## ۲. وضعیت: چه چیزی حل شده و چه چیزی مانده

| موضوع | وضعیت |
|---|---|
| قاب واقعی ویندوز دور اورب (title bar با minimize/maximize/close) که با هر minimize/restore برمیگشت | ✅ حل شد (گارد خودترمیم استایل؛ شاهد عددی دارد) |
| کادرهای شبح که با هر دیکته روی هم انباشته میشدند | ✅ حل شد (نگه داشتن HWND واقعی از eframe بهجای حدس با عنوان + پاککردن پنجرهٔ بازساختهشده) |
| تغییر شکل/وضعیت نمایش در هر مرز چانک | ✅ حل شد (state در تمام session روی `Recording` میماند) |
| قطعشدن ضبط با سکوت کوتاه | ✅ حل شد (`silence_ms` ۶۰۰→۱۲۰۰ و هیچ سکوتی session را تمام نمیکند) |
| **نوار/کادر روشن اضافی بالای اورب و دور باکس متن** | ❌ **باقی است (موضوع همین سند)** |

---

## ۳. محیط دقیق

| مورد | مقدار |
|---|---|
| OS | Microsoft Windows 11 Pro، build **26200** |
| مقیاس نمایش | `AppliedDPI = 120` ⇒ **۱۲۵٪** |
| GPU | NVIDIA GeForce RTX 4050 Laptop (driver 32.0.16.1656) + Intel UHD (hybrid) |
| رزولوشن | 1536×864 (مطابق خروجی probe) |
| Rust crates | `eframe/egui/egui-wgpu/egui-winit = 0.28.1`، `wgpu = 0.20.1`، `winit = 0.29.15`، `raw-window-handle = 0.6.2` |
| بکاند رندر | `Renderer::Wgpu` (Vulkan؛ لاگ راهاندازی آداپتر `NVIDIA GeForce RTX 4050` را انتخاب میکند) |
| توزیع | اپ از `v-2/voice-ptt-dist/voice-ptt.exe` اجرا میشود (کپی بیلد `cargo build --release`) |
| پروفایل | `Cargo.toml`: `default = ["silero-vad"]` (فیچر `light-theme` **فعال نیست**) |

---

## ۴. معماری پنجرهها در این اپ

نقشهٔ پنجرههای پروسه از `window-style-probe.ps1` (خروجی واقعی):

```
class=Window Class               vis=True  rect=(1370,104 162x162) style=0x96000000 ex=0x00040018 caption=False thickframe=False popup=True edged=False layered=False  <- اورب (پنجرهٔ اصلی eframe)
class=Winit Thread Event Target  vis=True  rect=(0,0 14x14)       style=0x94000000 ex=0x080800A0  (ساختهٔ خود winit؛ NX_LAYERED|TRANSPARENT|NOACTIVATE)
class=tray_icon_app              vis=False rect=(21,21 922x470)   (پنجرهٔ پیام tray)
class=wgpu Device Class …        vis=False rect=(0,0 133x38)
class=temp_d3d_window_…          vis=False rect=(0,0 1x1)
class=NVOpenGLPbuffer ×2         vis=False
class=IME ×5                     vis=False
```

سه viewport داخل یک پروسهٔ eframe وجود دارد (هر کدام یک پنجرهٔ OS جدا؛ همه با کلاس `Window Class`):

| viewport | کد | title | خصوصیات |
|---|---|---|---|
| اورب (اصلی) | `v-2/voice-ptt/src/lib.rs` خطوط 481–496 | `""` | transparent + frameless + always-on-top + non-resizable |
| داشبورد/مدیریت | `gui/overlay.rs` خط ۳۵۰۹ (`show_viewport_immediate`) | — | پنجرهٔ معمولی theme دار |
| پنجرهٔ رضایت ابری | `gui/overlay.rs` خط ۳۷۹۰ | — | پنجرهٔ معمولی theme دار |
| **باکس متن (رونویسی)** | `gui/overlay.rs` خطوط ۳۹۱۸–۴۰۰۶ (`render_preview_toast_window`) | **`OmniType_Preview`** | `show_viewport_immediate` + `with_decorations(false).with_transparent(true).with_always_on_top().with_resizable(false)`؛ موقعیت: پایینوسط بالای تسکبار |

نکات مهم معماری:

1. **پنجرهٔ باکس متن با `show_viewport_immediate` ساخته میشود**، یعنی هر فریم که «توست» فعال است، egui این viewport را
   بهصورت immediate میسازد/بهروزرسانی میکند و وقتی چیزی برای نمایش نیست، پنجره بسته میشود. عمر کارت ۱۰ ثانیه است.
2. **همهٔ راهبرد شفافیت** در تابع `enable_true_transparency` (`gui/overlay.rs:971`) جمع شده: حذف استایلهای فریم →
   تنظیم attributeهای DWM → `DwmExtendFrameIntoClientArea(-1,-1,-1,-1)` → `SetWindowPos(..., SWP_FRAMECHANGED)`.
3. `clear_color` (`gui/overlay.rs:4134`) همیشه `[0,0,0,0]` برمیگرداند (هم برای viewport اصلی، هم فرزندها).
4. `efframe`/`egui_wgpu`: در `egui-wgpu-0.28.1/src/winit.rs` (خطوط ۶۲۶–۶۳۶) رنگ پاککردن با
   `LoadOp::Clear(wgpu::Color{r,g,b,a})` اعمال میشود و برای alpha=0 → رنگ کاملاً شفاف. هیچ مسیر «بدون پاککردن» وجود ندارد.
5. چون در ویندوز کلاینت `hbrBackground = 0` (NULL) رجیستر میشود (winit 0.29.15، `platform_impl/windows/window.rs:1368`)،
   سیستم برای این پنجرهها **پسزمینه نقاشی نمیکند**.

### کدهای مربوط به استایل/شفافیت (برای مرجع مشاور)

| تابع | فایل:خط | نقش |
|---|---|---|
| `enable_true_transparency(hwnd)` | `gui/overlay.rs:971` | همهٔ حذف استایل + attributeهای DWM + extend frame + `SWP_FRAMECHANGED` |
| `window_style_is_shaped(style, ex)` | `gui/overlay.rs:1083` | تشخیص «قاب برگشته» بر اساس بیتهای style/ex |
| `enforce_frameless_window(hwnd)` | `gui/overlay.rs:1119` | گارد خودترمیم: ۲ خواندن `GetWindowLongW` در هر فریم؛ فقط در صورت drift، شکلدهی کامل |
| `force_repaint(hwnd)` | `gui/overlay.rs:1168` | `RedrawWindow(RDW_INVALIDATE\|RDW_ALLCHILDREN\|RDW_UPDATENOW)` (بدون ERASE/FRAME) |
| `register_main_hwnd(frame)` | `gui/overlay.rs:1287` | گرفتن HWND واقعی از `frame.window_handle()` و **مقایسه** در هر فریم (تشخیص پنجرهٔ بازساختهشده) |
| `shape_preview_window()` | `gui/overlay.rs:1342` | همان گارد برای پنجرهٔ `OmniType_Preview` (هر فریم، فقط وقتی کارت باز است) |
| `OrbWindow::place(...)` | `gui/orb.rs:654` | `SetWindowPos(HWND_TOPMOST, …)` فقط وقتی هندسه تغییر کرده + `force_repaint` بعد از جابهجایی موفق |
| `OverlayApp::update` | `gui/overlay.rs` (~۴۱۳۰) | هر فریم: `register_main_hwnd` → `enforce_frameless_window` (با لاگ هشدار تا ۱۰ بار) |

---

## ۵. تاریخچهٔ تغییرات (چه چیزی، چرا، کجا) — مهم برای مشاور

| فاز | تغییر | دلیل | فایلهای دخیل |
|---|---|---|---|
| ۰ | قفل دستآزاد با دوبار زدن کلید (`LatchPolicy`) | درخواست کاربر: بدون نگهداشتن مداوم کلید هم بشود ضبط کرد | `state/machine.rs`، `config/settings.rs`، `hotkey/listener.rs` |
| ۱ | جایگزینی «حدس HWND با عنوان پنجره» با `register_main_hwnd` از `frame.window_handle()`؛ تابع قدیمی `apply_window_shapes_all_legacy` از مسیر داغ خارج (کد باقی، بدون فراخوانی)؛ `SetWindowTextW("")` کامنت شد؛ فرستادن `ViewportCommand::InnerSize` فقط در صورت تغییر | کادرهای شبح: هر پنجرهٔ بدون عنوان (از جمله `Winit Thread Event Target` و `tray_icon_app`) اشتباهاً پنجرهٔ اپ فرض و به هندسهٔ اورب منتقل/ریسایز میشد | `gui/overlay.rs`، `gui/orb.rs`، `Cargo.toml` (`raw-window-handle`) |
| ۲ | قطع مسیر مردهٔ `egui_notify` (هیچجا `toasts.show()` نبود ⇒ `needs_animation_frames()` همیشه true ⇒ حلقهٔ ۳۰fps دائمی)؛ مستقلکردن کارت متن با سوئیچ `gui.show_transcript_bubble`؛ گیت پروب پسزمینهٔ Antigravity؛ گارد `asr.auto_local_fallback = false` | مصرف بالای RAM/CPU بدون اجرا شدن مدل لوکال + سوار بودن باکس متن روی اورب | `gui/overlay.rs`، `lib.rs`، `asr/*.rs`، `config/settings.rs` |
| ۳ | برداشتن سقف ۳۰ ثانیه (`ring_seconds` ۳۰→۶۰)، چانکبندی زماناجرا (`should_flush_chunk`, `take_chunk`, `process_chunk`) با همپوشانی ۳۰۰ms و بازگشت به `Recording` | قطع خودکار ضبط در ۳۰ ثانیه + درخواست پردازش چانکبهچانک | `state/machine.rs`، `config/settings.rs`، `audio/capture.rs` |
| ۳٫۱ | ماژول خالص `processing/seam.rs`: حذف کلمههای تکراری درز + پاککردن قطعهٔ کلمهٔ بریده با backspace (`inject_backspaces`) | کیفیت متن در مرز چانکها (کلمهٔ تکراری/بریده) | `processing/seam.rs`، `output/injector.rs`، `state/machine.rs` |
| ۳٫۲ | **الف)** گارد خودترمیم قاب (`enforce_frameless_window`) + مقایسهٔ HWND در هر فریم + شکلدهی هر فریم پنجرهٔ `OmniType_Preview`؛ **ب)** `AppStatus.chunk_busy` و حذف `set_state(Processing)` از `process_chunk`؛ **ج)** `silence_ms` ۶۰۰→۱۲۰۰. **در همین فاز یک `RedrawWindow(…\|RDW_ERASE\|RDW_FRAME\|…)` هم اضافه شد** | قاب ویندوز برمیگشت (شاهد در لاگ: `window frame drifted back; re-stripped caption/border repairs=1`)؛ تغییر شکل نمایش در هر چانک؛ سکوت کوتاه | `gui/overlay.rs`، `gui/orb.rs`، `state/machine.rs`، `config/settings.rs` |
| ۳٫۳ | حذف `RDW_ERASE` و `RDW_FRAME` از پرچمهای repaint و حذف کامل `RedrawWindow` از پایان `enable_true_transparency` | فرضیهٔ علتِ نوار روشن: `RDW_FRAME` روی پنجرهای که **کل کلاینتش ناحیهٔ فریم DWM** است، باعث میشود DWM آن ناحیه را با لایهٔ روشن گلس رنگ کند | `gui/overlay.rs` |

> **قاعدهٔ کاری حاکم بر این پروژه (خواستهٔ کاربر):** هر تغییر پرریسک **حذف نمیشود**، فقط غیرفعال/کامنت میشود تا
> مسیر rollback در درخت بماند. مثالها: `apply_window_shapes_all_legacy`، `SetWindowTextW("")`، `REDW_*` قبلی،
> گارد generation پنجرهٔ preview.

---

## ۶. شواهد اندازهگیریشده (همه با ابزارهای همین ریپو)

### ۶٫۱ قاب پنجره قبل/بعد از فاز ۳٫۲

```
قبل: class=Window Class style=0x16CB0000 ex=0x00040118 caption=True  thickframe=True  popup=False
بعد: class=Window Class style=0x96000000 ex=0x00040018 caption=False thickframe=False popup=True
```

خط لاگ که ثابت میکند eframe/winit *بعد از* شکلدهی اولیه استایل را برمیگرداند:

```
INFO  voice_ptt::gui::overlay: main window handle registered from eframe's raw window handle hwnd=25496724
WARN  voice_ptt::gui::overlay: window frame drifted back; re-stripped caption/border repairs=1 hwnd=25496724
```

### ۶٫۲ اندازهگیری پیکسلی نوار روشن در اسکرینشات کاربر

فایل: `…\Pictures\Screenshots\Screenshot 2026-09-29 052555.png` (کراپ ۴۴۳×۳۳۶) و paste الحاقی.

```
نوار روشن: y=33..60 (ارتفاع ۲۸) ، x=70..272 (عرض ۲۰۳) ، میانگین رنگ (220,235,250)
اسکن افقی روی y=46:
   x=60  -> (34,37,42)      ← پسزمینهٔ تیره
   x=70  -> (225,242,255)   ← لبهٔ تیز، شروع نوار
   x=96+ -> (223,240,255)، x=139 -> (221,236,254) …  ← گرادیان ملایم، کانال آبی ≈۲۵۵
دور کارت متن (اسکرینشات دیگر): ۱۵٬۰۳۶ پیکسل با (255,255,255) و همچنین (249,249,249)/(248,248,248) ⇒ سطح سفید مات
```

نتیجهٔ دو مشاهدهای: **لبهٔ تیز + سطح داخلی گرادیانی + آبی اشباع** ⇒ یک سطح مات/نیمهگلس که توسط سیستم یا
کامپوزیتور رنگ شده، نه یک نقاشی egui با قلمهای نیمهشفاف.

### ۶٫۳ اندازهگیری خودکار بعد از فاز ۳٫۳ (توسط همین ایجنت، لحظهٔ idle)

```
داخل نوار بالای پنجرهٔ اورب (y = top+20):  255,255,255
بیرون پنجره (بالا/چپ/پایین):              255,255,255 / 255,255,255 / 252,252,252
```

یعنی در آن لحظه و در حالت idle، ناحیهٔ بدوننقاشی پنجرهٔ اورب **شفاف** بود و پسزمینهٔ پشتش دیده میشد
(پس نوار در آن نمونه **دیده نشد**). ولی کاربر میگوید در استفادهٔ واقعی هنوز نوار را میبیند. این ناسازگاری
خودش یک سرنخ است: آرتیفکت **وضعیتمحور/گذارا** است (احتمالاً هنگام/پس از دیکته یا در لحظهٔ رخداد
`SetWindowPos`/`SWP_FRAMECHANGED`/minimize-restore).

---

## ۷. فرضیههای باقیمانده (به ترتیب احتمال) + آزمایش تفکیککننده

### H1 — ناحیهٔ «فریم گستردهشده» DWM با لایهٔ روشن پر میشود (احتمال بالا)
پنجره با `DwmExtendFrameIntoClientArea(-1,-1,-1,-1)` ساخته میشود، یعنی **کل کلاینت از دید DWM «فریم» است**.
هر رخدادی که DWM را به بازترسیم فریم وادار کند (تغییر استایل + `SWP_FRAMECHANGED`، `RDW_FRAME`،
minimize/restore، تغییر تم/backdrop، جابهجایی بین مانیتورها) میتواند آن ناحیه را با لایهٔ گلس/روشن
(سفید مایل به آبی، احتمالاً همان `224,241,255`) پر کند؛ چون egui هر فریم فقط پیکسلهای خودش را مینویسد و
بقیهٔ ناحیه در سواپچین alpha=0 است، نتیجه «مستطیل روشن» دیده میشود.
*موافق:* رنگ و بافت (سطح یکنواخت با گرادیان ملایم، لبهٔ تیز)، شروع مشکل بعد از افزودن `RDW_FRAME`،
وابستگی به minimize/restore.
*مخالف:* بعد از حذف `RDW_*` هم کاربر میگوید باقی است ⇒ یک trigger دیگر (مثلاً `SWP_FRAMECHANGED` در
`enable_true_transparency` یا خودِ extend-frame) باقی است.
*آزمایش:* (الف) مقدار `DwmExtendFrameIntoClientArea` را با `(0,0,0,0)` تست کنیم و ببینیم نوار میرود یا نه
(اگر برود ⇒ تأیید H1)؛ (ب) `RDW_FRAME` را دوباره اضافه کنیم و ببینیم فوراً برمیگردد (اثبات علیت)؛
(ج) در GDI/DWM، مقدار `DWMWA_NCRENDERING_POLICY` را به `ENABLED` ببریم تا ببینیم رفتار عوض میشود.

### H2 — سواپچین alpha ندارد یا `alpha_mode` سطح اشتباه است (احتمال متوسط)
اگر سطح Vulkan با فرمت بدون آلفا (`Bgra8Unorm` بدون `_SRGB` با `alpha_mode = Opaque`) ساخته شود، رنگ
`(0,0,0,0)` بهصورت مات نوشته میشود؛ ولی آنوقت انتظار «مشکی» داریم نه «سفید مایل به آبی». بااینحال
اگر ناحیهٔ بدوننقاشی هرگز نوشته نشود، محتوای قبلی/ناشناختهٔ بافر دیده میشود که رنگش میتواند سفید باشد.
*آزمایش:* لاگ فرمت سطح و `alpha_mode` را از `wgpu` بگیریم (`surface.get_capabilities(&adapter)` در
`WgpuConfiguration`/`SurfaceConfiguration`)، و یک تست مینیمال egui+wgpu با همان flagها بسازیم و ببینیم
ناحیهٔ بدوننقاشی در آن شفاف است یا سفید.

### H3 — یک آرتیفکت مستقل از پنجره (پنجرهٔ سومشخص / overlay دیگر) (احتمال متوسط)
روی این دستگاه چند لایهٔ تمامصفحه فعال است: `Cua.AgentCursorOverlay`، `Windows Input Experience`،
`NVIDIA GeForce Overlay (CEF-OSC-WIDGET)`، و یک پنجرهٔ `WindowsForms10.Window…` با rect `(1206,18 314x62)`
و `layered=True` که عنوان ندارد. هر کدام میتوانند یک نوار روشن بکشند.
*آزمایش:* اسکریپت `artifact-probe.ps1` را **در لحظهای که نوار دیده میشود** اجرا کنید؛ خروجی، فهرست پنجرههای
روی آن نقطه به ترتیب z-order را میدهد و اولین ردیف = مالک پیکسلها.

### H4 — گرادیان/بلور باقیمانده در سطح egui (احتمال کم)
کارت متن خودش `rect_filled` با آلفا میکشد (سایههای مشکی) و پنل مرکزی شفاف است؛ برای سطح سفیدِ مات توضیحی
ندارد مگر اینکه `visuals` فرزند اعمال نشود و egui با تم روشن رندر کند.
*آزمایش:* در همان viewport، `toast_ctx.style()` را لاگ/اسکرینشات کنیم و مطمئن شویم `panel_fill = TRANSPARENT`.

### H5 — گردی/سایهٔ DWM یا `DWMWA_BORDER_COLOR` روی ویندوز ۱۱ ۲۵H2 (احتمال کم)
نسخهٔ build 26200 رفتار جدیدی برای `DWMWA_SYSTEMBACKDROP_TYPE=NONE` + extend-frame دارد.
*آزمایش:* حذف موقت `DWMSBT_NONE` و تست `DWMSBT_AUTO`؛ و حذف موقت `DwmExtendFrameIntoClientArea` و
استفاده از `WS_EX_LAYERED` + `UpdateLayeredWindow` (رویکرد کلاسیک شفافیت) بهعنوان جایگزین.

---

## ۸. پروتکل بازتولید و ابزارها

### ۸٫۱ لحظهای که نوار دیده میشود

```bash
cd v-2
# مالک پیکسلهای روشن را نام میبرد (فهرست z-order + style بیتها)
powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File docs/reaserch/gui/probes/artifact-probe.ps1 -SaveCapture bar.png
```

خروجی نمونه (ساختار):

```
== around orb window: region (1250,0) 402x402
   band: screen y=22..23 (h=2) x=1336..1568 (w=233)
         colours left/mid/right = (199,199,199) (40,40,40) (95,95,95)
         owner windows at centre (1452,22), z-order top to bottom:
            z=32   pid=29456  rect=(…) style=0x… ex=0x… caption=… popup=… layered=… class=… title="…"
            … (اولین ردیف = پنجرهٔ رویی روی آن نقطه)
```

### ۸٫۲ سایر ابزارها

| ابزار | کار |
|---|---|
| `docs/reaserch/gui/probes/window-probe.ps1` | هندسهٔ همهٔ پنجرههای پروسه + حافظه/هندل/ترد |
| `docs/reaserch/gui/probes/window-style-probe.ps1` | بیتهای استایل (`caption/thickframe/popup/edged/layered`) هر پنجره + private memory |
| `docs/reaserch/gui/probes/dictation-report.ps1` | تحلیل لاگ: طول session، چانکها، ترمیم درز، لود مدل لوکال، خطاها |
| `docs/reaserch/gui/probes/artifact-probe.ps1` | یافتن نوار روشن و **نام بردن مالک پنجرهاش** |

### ۸٫۳ لاگ

```
%APPDATA%\voice-ptt\logs\voice-ptt.log.YYYY-MM-DD      (UTC)
```
خطوط کلیدی: `main window handle registered…`، `window frame drifted back; re-stripped caption/border repairs=N`،
`flushing mid-session chunk … still_recording=true`، `chunk text ready … dropped=… backspaces=…`،
`chunk seam repaired …`.

---

## ۹. سؤالهای مشخص از مشاور خارجی

1. روی ویندوز ۱۱ (build 26200) با `wgpu/Vulkan`، آیا ترکیب
   «`WS_POPUP` + `WS_EX_*` تمیزشده + `DwmExtendFrameIntoClientArea(-1)` + `DWMWA_SYSTEMBACKDROP_TYPE=NONE` +
   سواپچین alpha=0» بهصورت تضمینی ناحیهٔ نقاشینشده را شفاف نگه میدارد؟ چه چیزی میتواند آن را به سطح
   سفید/روشن تبدیل کند؟
2. آیا `SetWindowLongW(GWL_STYLE, …)` + `SWP_FRAMECHANGED` روی پنجرهای با extend-frame، خودش trigger
   بازترسیم ناحیهٔ فریم DWM با رنگ گلس است؟ اگر بله، ترتیب/روش درست اعمال چه است (مثلاً اعمال attributeها
   **قبل از** نشاندادن پنجره، یا استفاده از `WM_NCCALCSIZE` سفارشی بهجای حذف استایل)؟
3. برای یک overlay شفافِ همیشه-رو، آیا راهبرد توصیهشده روی ویندوز ۱۱ این است:
   (الف) `DwmExtendFrameIntoClientArea` + alpha سواپچین، (ب) `WS_EX_LAYERED` + `UpdateLayeredWindow`،
   (ج) `DirectComposition`/`WS_EX_NOREDIRECTIONBITMAP`، یا (د) `Acrylic/Mica` با backdrop؟ کدام پایدارتر است
   با `eframe 0.28 + wgpu 0.20`؟
4. برای پنجرهٔ دوم (کارت متن) که با `show_viewport_immediate` هر فریم بهروزرسانی موقعیت/اندازه میشود،
   آیا «مستطیل روشن/سفید» میتواند ناشی از همین churn باشد؟ آیا باید به `show_viewport_deferred`
   (پنجرهٔ پایدار + `ViewportCommand::Visible`) مهاجرت کنیم؟
5. اگر علت DWM است، آیا راهحل حداقلی وجود دارد که *هیچ* لایهٔ روشنی نگذارد
   (مثلاً `DWMWA_NCRENDERING_POLICY=ENABLED` برای غیرفعالکردن رندر ناحیهٔ غیرکلاینت، یا
   `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, DONOTROUND)` + حذف extend-frame)؟

---

## ۱۰. چیزهایی که بررسی/رد شدهاند (تا مشاور دوباره وقت نگذارد)

- **قاب واقعی ویندوز** (`WS_CAPTION`) — پوشش داده شد و حل شد (فاز ۳٫۲).
- **کادرهای شبح انباشته** — علت: حدس HWND با عنوان خالی؛ حل شد (فاز ۱).
- **رنگ پنل egui** — `panel_fill` در بیلد پیشفرض تیره است (`palette::WINDOW_BG = (18,20,28)`) و
  پنجرهٔ اورب هیچ `CentralPanel` ندارد؛ رنگ مشاهدهشده `(224,241,255)` با این مقدار نمیخواند.
- **لیبل استایل winit** — `hbrBackground = 0` (NULL) است؛ سیستم برای این پنجره پسزمینه نمیکشد.
- **crates** — `egui-wgpu 0.28.1` همیشه `LoadOp::Clear` با همان `clear_color` میزند؛ مسیر «بدون پاککردن» وجود ندارد.
- **`RDW_ERASE`/`RDW_FRAME`** — یک بار بهعنوان علت شناسایی و حذف شد؛ ولی گزارش کاربر میگوید آرتیفکت باقی است
  ⇒ احتمالاً **یک** عاملِ دیگر هم هست (H1/H2/H3).

---

## ۱۱. پیوست: فایلهای تغییریافته در این مجموعهٔ کار

```
 M voice-ptt/Cargo.toml, Cargo.lock
 M voice-ptt/README.md
 M voice-ptt/src/asr/{engine.rs, router.rs, whisper.rs}
 M voice-ptt/src/audio/capture.rs
 M voice-ptt/src/config/settings.rs
 M voice-ptt/src/gui/{orb.rs, overlay.rs}
 M voice-ptt/src/hotkey/listener.rs
 M voice-ptt/src/lib.rs
 M voice-ptt/src/output/{injector.rs, mod.rs}
 M voice-ptt/src/processing/mod.rs
 M voice-ptt/src/state/machine.rs
 M voice-ptt/src/vad/mod.rs
?? voice-ptt/src/processing/seam.rs
?? docs/GUI-BUGFIX-PLAN.md
?? docs/GUI-WINDOW-ARTIFACT-REPORT.md   (همین سند)
?? docs/reaserch/gui/probes/{window-probe.ps1, window-style-probe.ps1, dictation-report.ps1, artifact-probe.ps1}
```

وضعیت راستیآزمایی در آخرین بیلد: `cargo clippy --all-targets` بدون هشدار · `cargo test` = ۱۸۳ lib + ۵ + ۳ + ۴ + ۴ سبز.
