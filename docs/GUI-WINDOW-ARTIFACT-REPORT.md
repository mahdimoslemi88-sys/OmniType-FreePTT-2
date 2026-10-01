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
---

## ۱۲٫۸ تغییرات این بخش

- `voice-ptt/src/gui/overlay.rs`: `TransparencyMode` + `transparency_mode_from`/`transparency_mode` (متغیر محیطی،
  **پیش‌فرض = `swapchain`**)، شاخهٔ `swapchain` در `enable_true_transparency`، شرطی‌شدن `SWP_FRAMECHANGED`،
  `log_window_geometry` (یک‌بار هنگام ثبت HWND: `dpi`، `window_w/h`، `client_w/h`، `mode`) و دو تست برای قفل
  نگه‌داشتن نگاشت. پیش‌بینی لاگ: `dpi=120 client_w=203 client_h=203 ppp=1.25`.
- `docs/reaserch/gui/probes/artifact-probe.ps1` و `window-style-probe.ps1`: `SetProcessDpiAwarenessContext(-4)`
  قبل از هر فراخوانی، به‌علاوهٔ چاپ `dpi` هر پنجره. (این همان باگی است که اعداد §۳ و §۴ را خراب کرده بود.)
- **ریفکتور:** کل منطق Win32/DWM از `overlay.rs` به ماژول مستقل `gui/window_shape.rs` منتقل شد
  (`overlay.rs` از ۵۰۰۶ به ۴۳۵۳ خط، `window_shape.rs` = ۷۱۰ خط). دلیل: این کد باید در برابر کامپوزیتور ویندوز
  استدلال شود نه در برابر چیدمان UI، و دیگر وسط یک فایل ۵۰۰۰ خطی پنهان نیست. تست‌های شفافیت هم همراهش رفتند.
- **پنجرهٔ اورب دیگر resize نمی‌شود**: یک‌بار در بزرگ‌ترین اندازه (`Orb::max_canvas_points`) ساخته می‌شود و
  انیمیشن فقط داخل آن اتفاق می‌افتد ⇒ نوار یخ‌زدهٔ ۲۹۸×۲۸ دیگر ساخته نمی‌شود.
- **پنجرهٔ کارت متن حالا موقع ساخت شکل می‌گیرد**: `shape_preview_window` فقط گاردِ drift بود و چون بیت‌های
  استایل پنجرهٔ تازه سالم‌اند، `enable_true_transparency` **هرگز** رویش اجرا نمی‌شد.
- ابزارهای جدید: `screenshot-measure.ps1` و `artifact-repro.ps1`.

---

## ۱۳. تشخیص نهایی: `with_transparent(true)` در ویندوز ۱۱

> **وضعیت سند:** بخش‌های ۱ تا ۱۲٫۷ آخرین بار همراه با پروبه‌ها commit نشده بود و بازنویسی این فایل آن‌ها را
> پاک کرد؛ متن بالا آخرین نسخهٔ commit‌شده به‌علاوهٔ ۱۲٫۸ است. اندازه‌گیری‌های میانی §۱۲ با اجرای دوبارهٔ
> پروب‌ها از `docs/reaserch/gui/probes/` قابل بازتولیدند.

### ۱۳٫۱ چه چیزی ثابت شد

سه مشاهدهٔ کاربر که تشخیص‌های قبلی را کنار گذاشت:

1. کادر روشن **کلیک را می‌بلعد** ⇒ رندر نمی‌تواند کلیک بدزدد، پس یک `HWND` زنده است.
2. با بزرگ شدن متن به **سمت چپ** می‌پرد ⇒ پنجره‌ای با اندازهٔ قدیمی زیر پنجرهٔ فعلی مانده.
3. **هم برای اورب هست هم برای کارت متن** ⇒ یک علت مشترک، نه دو مسیر متفاوت.

نکتهٔ ۳ کلیدی بود: هر دو پنجره از یک مسیر ساخته می‌شوند.

### ۱۳٫۲ زنجیرهٔ علت (از روی سورس، نه حدس)

```
preview_window.rs:  ViewportBuilder::with_transparent(true)
   └─ egui-winit-0.28.1/src/lib.rs:1595      .with_transparent(transparent.unwrap_or(false))
        └─ winit-0.30.13/.../windows/window.rs:1231-1246   (fn on_create)
             if attributes.transparent {
                 let region = CreateRectRgn(0, 0, -1, -1);          // ناحیهٔ خالی
                 DwmEnableBlurBehindWindow(hwnd, {
                     dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,      // ← قاتل
                     hRgnBlur: region, .. });
             }
```

از build **۲۲۶۲۱** به بعد (این دستگاه: **۲۶۲۰۰**)، `DWM_BB_ENABLE` دیگر یعنی «blur این ناحیه»
نیست؛ یعنی **system backdrop پنجره را روشن کن**. ناحیهٔ خالی ⇒ «backdrop روی تمام پنجره».

نتیجه: پنجرهٔ کارت (always-on-top، و آن ناحیه هیچ‌وقت رندر نمی‌شود) یک پنل روشن به اندازهٔ کل
client area است. چون یک **پنجره** است، کلیک هم می‌بلعد. گوشه‌های گرد/تیز هم از
`DWMWA_WINDOW_CORNER_PREFERENCE` است که هرگز روی این پنجره اعمال نشده بود.

رنگ `rgb(248,248,248)` هم تصادفی نبود: رنگ سیستمی ویندوز نیست (آن ۲۴۰ است) — این دقیقاً
`egui::Visuals::light().panel_fill` است، یعنی رنگی که کانتکست فرزند پیش‌فرض با آن شروع می‌شود.
الزامِ `set_visuals` قبل از هر رسمی، و frame شفاف، هر دو در `report_window` صریح شدند.

### ۱۳٫۳ چرا «فقط اصلاحش کن» کافی نبود

چون اصلاح = مسابقه با `on_create` در هر بار ساخت پنجره، برای همیشه. و دقیقاً همین اتفاق افتاده بود:
HWND پنجرهٔ کارت **هرگز** به هیچ‌کدام از کدهای Win32 نمی‌رسید (فقط پنجرهٔ ریشه از
`register_main_hwnd` می‌رفت)، پس هیچ‌چیز آن را اصلاح نمی‌کرد.

### ۱۳٫۴ چرا حذفش چیزی را خراب نمی‌کند

آلفای swapchain از `ViewportBuilder` کارت **نمی‌آید**:

```
eframe-0.28.1/src/native/wgpu_integration.rs:196
    egui_wgpu::winit::Painter::new(.., native_options.viewport.transparent)
```

این Painter **یک‌بار** در استارتاپ و از روی تنظیم **پنجرهٔ ریشه** ساخته می‌شود و
`Painter::add_surface` (egui-wgpu-0.28.1/src/winit.rs:274-276) همان یک فیلد را برای **همهٔ**
viewportها — از جمله فرزند — به کار می‌برد. ریشه در `lib.rs` `with_transparent(true)` دارد، پس سطح
کارت از قبل `CompositeAlphaMode::PreMultiplied` بود. فلگ روی فرزند فقط **اثر جانبی** بود، بدون فایده.

### ۱۳٫۵ تغییرات

- `gui/preview_window.rs` — بازنویسی کامل:
  - `with_transparent(true)` **حذف شد** (تست `card_window_does_not_ask_winit_for_transparency` آن را قفل می‌کند).
  - `with_mouse_passthrough(true)` اضافه شد: پنجرهٔ ثابت ۴۵۲×۲۶۰ نقطه‌ای است و حتی اگر کاملاً شفاف باشد،
    تا وقتی always-on-top است همان مستطیل را از دسکتاپ می‌گیرد — همان چیزی که به‌صورت «دکمهٔ کنارش کلیک
    نمی‌شود» گزارش شده بود. هزینه‌اش از دست رفتن click-to-dismiss است؛ با ثابت
    `CARD_CLICKS_PASS_THROUGH` یک‌خطی برمی‌گردد.
  - یک پنجره برای کل عمر پروسه؛ `with_visible` تنها چیزی است که بین بابل‌ها فرق می‌کند (تست
    `show_and_hide_differ_only_in_visibility`).
  - `window_position` به `window_shape::taskbar_bottom_center_pt` سپرده شد (کار درست، به‌جای ریاضیِ
    `screen_h - taskbar` که پنجره را زیر نوار وظیفه می‌برد).
  - بلوک مردهٔ `LayoutJob` در `paint_card` که ساخته و دور ریخته می‌شد حذف شد.
- `gui/window_shape.rs`:
  - `apply_viewport_transparency(hwnd)`: **تنها** اصلاح Win32 برای هر پنجرهٔ viewport — چهار فراخوانی
    idempotent، بدون نوشتن style و بدون `SWP_FRAMECHANGED` (برخلاف گارد قبلی که ۲۲۶۷ بار در ۱۶ ثانیه
    style می‌نوشت و همان چیزی بود که caption را برمی‌گرداند).
  - `shape_preview_window` → `ensure_preview_window_shaped`: پنجرهٔ کارت با **عنوان خودش** پیدا و تا
    عمرش اصلاح می‌شود (هر ۱۵ فریم یک‌بار + بلافاصله بعد از ساخته‌شدن).
- تست‌ها: ۸ تست `preview_window` (شامل قفل‌های regression روی `transparent` و `mouse_passthrough`)،
  ۱۹۴ تست کتابخانه سبز، `cargo clippy --all-targets` بدون هشدار.

### ۱۳٫۶ مسیرهای مرده (ثبت می‌شوند تا دوباره امتحان نشوند)

`DwmExtendFrameIntoClientArea` · همهٔ attributeهای DWM به‌جز backdrop/corner/border · subclass
`WM_NCCALCSIZE` · `SWP_FRAMECHANGED` · `DWMNCRP_DISABLED` · blur-behind (تنها به‌عنوان **پاک‌سازی** لازم
است، نه فعال‌سازی) · گارد per-frame روی `SetWindowLongW`.

### ۱۳٫۷ نکتهٔ عملی برای تست

`voice-ptt-dist/voice-ptt.exe` قبلاً ساعت ۰۸:۵۰ بود در حالی که سورس ۰۹:۱۰ — یعنی چند دور تست روی
بیلدی انجام شده که اصلاحات قبلی را نداشت. قبل از قضاوت دربارهٔ نتیجه، مطمئن شو که exe را از
`voice-ptt-dist/` اجرا می‌کنی. md5 بیلد فعلی در پیام تحویل آمده است.

---

## ۱۴. کارت متن حذف شد (به‌جای درست‌کردن)

### ۱۴٫۱ چرا

پس از رفع کادر روشن، دو مسئلهٔ باقی‌مانده بود: **کلیک‌دزدی** و **متن بیرون از کادر**. کلیک‌دزدی
ریشه‌یابی و حل شد (بخش ۱۴٫۵). متن بیرون از کادر هم ریشه داشت (`pos.x` به‌جای لبهٔ راست، لبهٔ چپ
بود و چون `halign = RIGHT` است، متن به اندازهٔ عرضش به چپ می‌افتاد). اما با توجه به اینکه کارت یک
**viewport بومی egui** است — یعنی یک `HWND` واقعی، always-on-top، که هر بازی DWM و هر تغییر
`WS_EX_*` روی آن، مسئلهٔ تازه‌ای می‌سازد — تصمیم گرفته شد به‌جای جنگیدن با آن، **اصلاً ساخته نشود**.

متن دیکته همچنان تایپ می‌شود و در تاریخچهٔ داشبورد می‌ماند؛ فقط تکرارِ شناورِ آن حذف شده است.

### ۱۴٫۲ پیاده‌سازی

یک قفل، نه حذف کد:

```rust
// gui/overlay.rs
const SHOW_TRANSCRIPT_CARD: bool = false;
```

`render_preview_toast_window` با آن به‌صورت زودهنگام `return` می‌کند و `live_toasts` را خالی
می‌کند (تا با برگرداندن قفل، بک‌لاگ حباب‌های کهنه زنده نشود). `report_window` صدا زده نمی‌شود، پس
**نه viewportی ساخته می‌شود، نه `HWND`ای**. `ensure_preview_window_shaped` هم چون فقط از
`report_window` صدا زده می‌شد، دیگر اجرا نمی‌شود. کل ماژول `preview_window` دست‌نخورده و تست‌شده
باقی مانده؛ برگرداندن کارت = `true`.

### ۱۴٫۳ اثبات اندازه‌گیری‌شده

پروب [click-thief-probe.ps1](reaserch/gui/probes/click-thief-probe.ps1) روی بیلد در حال اجرا:

```
WINDOW      RECT        W    H   REGION        VERDICT
(untitled)  -30,812   298  298  76,76 146x146  THIEF - owns its centre
    corners=[none  other:BrokenArrow  none  none]
(untitled)  0,0        18   18  (whole rect)  ok - click-through
```

پنجرهٔ `OmniType_Preview` **در فهرست نیست** — یعنی اصلاً وجود ندارد. تنها پنجرهٔ بزرگ، اورب است
و حالا ناحیهٔ دایره‌ای ۱۴۶×۱۴۶ پیکسل دارد که گوشه‌هایش به دسکتاپ می‌رسند (قبلاً
`corners=[SELF SELF SELF SELF]` بود).

### ۱۴٫۴ تله‌ای که همین‌جا گرفته شد

`ClickRegion` بر حسب **نقطه** است، ولی شعاعِ محاسبه‌شده در `orb.rs` بر حسب **پیکسل** بود و با
`ppp = 1.0` تحویل داده می‌شد. روی این نمایشگر ۱٫۲۵ در این حالت شعاع ۱٫۲۵ برابر کوچک‌تر می‌شد و
هالهٔ اورب **بریده** می‌شد. اعداد تصادفاً برابر بودند چون `region_radius_px` از قبل ضرب در `ppp`
شده بود — اما این یک تصادف بود، نه یک قرارداد. حالا `place` پارامتر `ppp` می‌گیرد و تبدیل واحد
صریح است.

### ۱۴٫۵ دزد کلیک: پنجرهٔ اورب (همان بیلد، جدا از کارت)

پروب ثابت کرد کارت **دیگر** کلیک نمی‌دزد (`WS_EX_TRANSPARENT` دارد و در هر پنج نقطهٔ
نمونه‌برداری، `WindowFromPoint` پنجرهٔ زیرین را برمی‌گرداند). دزد، پنجرهٔ اورب بود:

```
max_canvas_points() = canvas_side_points(Recording.target_scale()) = 238 نقطه
→  238 × 1.25 = 298 پیکسل مربع، always-on-top، بدون ناحیه، بدون passthrough
→  88,804 پیکسل دسکتاپ که بلعیده می‌شد
```

`WS_EX_TRANSPARENT` برای اورب جواب نمی‌دهد چون all-or-nothing است و اورب باید کشیده‌شدنی بماند
(`ui.interact(..., Sense::click_and_drag())`). و `WM_NCHITTEST` → `HTTRANSPARENT` هم بی‌فایده است:
طبق مستندات MSDN فقط پنجره‌های **هم‌ترد** را رد می‌کند، و پنجره‌هایی که باید محافظت شوند پروسهٔ
دیگری‌اند. تنها راه باقی‌مانده **ناحیهٔ پنجره** (`SetWindowRgn`) است.

شعاع ناحیه از روی کد، نه حدس، حساب می‌شود (`Orb::painted_radius_pt`):
`radius × 1.10` (تنفس) × `(1 + GLOW_EXTENT)` (هاله) و `+ radius × SHAKE_EXTENT` (لرزش).

نکته: `SetWindowRgn` **هم رندر را می‌بُرد، هم hit-test**. اگر شعاع ناحیه از شعاع هالهٔ نقاشی‌شده
کوچک‌تر باشد، گوشه‌های هاله با لبهٔ سخت بریده می‌شوند — به همین دلیل فرمول بالا یک سقف صریح
است و تست `region_never_crops_the_glow` هم آن را قفل می‌کند.

---

## ۱۵. هالهٔ سفید بالای سر اورب (کمان یخ‌زده)

### ۱۵٫۱ اندازه‌گیری، نه حدس

کمان سفید بالای اورب در تصویر کاربر:

| کمیت | مقدار |
|---|---|
| ردیف‌ها | ۴۵۴ تا ۴۶۳ (۱۰ پیکسل، با لبهٔ تیز بالا و پایین) |
| رنگ | `(214,228,242)`، ردیف آخر `(255,255,255)` |
| مرکز | هم‌مرکز با اورب: `x=1511.5`، `y≈574` |
| شعاع | **۱۲۰٫۴ پیکسل** (برازش دایره از دو مقطع) |
| پهنا در `y=455` / `y=463` | ۳۴ / ۹۲ پیکسل — یعنی واقعاً کمانِ یک دایره |
| فاصله تا پس‌زمینه | ۲۳۰ واحد روشنایی روی پس‌زمینهٔ ۲۵ |

و `painted_radius_pt(Recording)` در [orb.rs](../voice-ptt/src/gui/orb.rs) دقیقاً
`96.25 pt × 1.25 = 120.3 px` می‌دهد. **تطابق تا ۰٫۱ پیکسل.** این لبهٔ ناحیهٔ کلیکِ بزرگ است.

### ۱۵٫۲ ریشه

پنجرهٔ اورب یک پنجرهٔ ثابت ۲۹۸×۲۹۸ است که هرگز resize نمی‌شود، ولی ناحیهٔ کلیکش با انیمیشن
بالا و پایین می‌رود: ۵۸ نقطه در idle، ۹۶ نقطه در Recording.

`SetWindowRgn(hwnd, rgn, TRUE)` تمام کاری که می‌کند این است که `WM_WINDOWPOSCHANGED` بفرستد.
ولی DWM یک پنجرهٔ per-pixel-alpha را از **سطح بازترسیم‌شدهٔ کش‌شده** ترکیب می‌کند و آن پیام
آن سطح را باطل نمی‌کند. پس وقتی ناحیه کوچک می‌شود (بازگشت اورب به idle)، DWM همچنان آخرین
فریمی را نشان می‌دهد که با ناحیهٔ بزرگ‌تر ترکیب شده بود — یعنی دایرهٔ ۱۲۰ پیکسلی.

چرا فقط بالا؟ چون بقیهٔ دایره با محتوای خود اورب هم‌پوشانی دارد و دیده نمی‌شود؛ لبهٔ بالا در
فضای خالیِ بالای سر می‌افتد.

### ۱۵٫۳ اصلاح

`apply_click_region` حالا بعد از هر `SetWindowRgn` موفق، `force_repaint(hwnd)` صدا می‌زند:
`RedrawWindow(INVALIDATE | ALLCHILDREN | UPDATENOW)` — همان ترکیبی که از قبل برای جابه‌جایی
پنجره استفاده می‌شد و امن است.

`RDW_ERASE` و `RDW_FRAME` **عمداً** اضافه نشده‌اند: خواستن از DWM برای بازترسیم ناحیهٔ فریمِ
یک پنجرهٔ شفاف، به‌صورت یک لایهٔ روشن روی بخش‌هایی که egui هرگز نقاشی نمی‌کند فرود می‌آید —
همان کادر روشنی که این ماژول برای از بین بردنش ساخته شده.

### ۱۵٫۴ پروب تشخیص خودکار

[orb-ghost-check.ps1](reaserch/gui/probes/orb-ghost-check.ps1) بعد از هر دیکته اجرا می‌شود و
خودش تصمیم می‌گیرد:

```
powershell -File orb-ghost-check.ps1
# exit 0 = پاک، exit 1 = هاله هست
```

روی تصویری که آرتیفکت در آن بود:

```
orb rect 1363,430 298x298  centre 1512,579
background 25
VERDICT: GHOST ARC PRESENT
Rows     Px Dist Rise
----     -- ---- ----
454..463 10  120  230
```

و روی یک تصویر سالم: `VERDICT: clean`.

معیار تشخیص سه شرط هم‌زمان است: نوار باریک (≤۲۴ پیکسل)، لبهٔ تیز (ردیف بالایی برگشته به پس‌زمینه)،
و بیرون از ۹۰ پیکسلی مرکز اورب. یک درخشش نرم هر سه شرط را رد می‌کند.

### ۱۵٫۵ اگر برگشت

پلن جایگزین که عمداً اجرا نشده: **ناحیه را اصلاً کوچک نکن**. یک‌بار روی بیشترین شعاع ممکن
(۹۶ نقطه) تنظیم کن و دیگر عوضش نکن. هزینه‌اش این است که در idle هم ۲۴۰ پیکسل قطر کلیک می‌گیرد
به‌جای ۱۴۶ — یعنی ۴۵٬۲۳۹ پیکسل به‌جای ۱۶،۷۳۶. مزیتش این است که هیچ تغییر ناحیه‌ای رخ نمی‌دهد و
این دسته از آرتیفکت‌ها اساساً ممکن نیست.

---

## ۱۶. وضعیت نهایی: آرتیفکت **وجود دارد** و محرکش شناسایی شد

> تاریخ: ۲۰۲۶-۰۹-۳۰. بخش‌های ۱–۱۵ تاریخچهٔ این گزارش‌اند. این بخش **خلاصهٔ نهایی** است و
> هر چیزی را که قبلاً گفته شده باطل می‌کند، صریح باطل اعلام می‌کند.

### ۱۶٫۱ اندازه‌گیری قطعی

روی اسکرین‌شات تمام‌صفحه با پس‌زمینهٔ تیره (`25,25,25`)، برازش کمترین‌مربعات دایره روی لبهٔ باند
(بیشترین انحراف **۰٫۸px در هر ۱۰ ردیف**):

| کمیت | مقدار |
|---|---|
| شعاع کمان | **۹۶٫۶pt = ۱۲۰٫۸px** |
| ضخامت | **۱۰px** (ردیف ۲۳۱..۲۴۰) |
| رنگ | `(215,229,243)` → در ردیف آخر `(255,255,255)` خالص |
| لبه | **سخت** (ردیف ۲۳۰ و ۲۴۱ صفر پیکسل روشن) |
| هم‌مرکزی با هستهٔ اورب | **۰٫۲px** اختلاف |

`region_radius_pt` در حالت Recording = **۹۶٫۲۵pt**. اختلاف **۰٫۴۴px** — در حد خطای برازش.

### ۱۶٫۲ محرک: **جابه‌جایی پنجره**، نه حالت ضبط

گزارش قبلی فرض می‌کرد آرتیفکت مخصوص گذار به Recording است. **ناظر این را رد کرد:**

1. در لحظهٔ بالا آمدن برنامه **وجود ندارد**.
2. بعد از چند بار کلیک/کشیدن اطراف اورب **ظاهر می‌شود**.
3. **همراه اورب حرکت می‌کند** و با بزرگ/کوچک شدن اورب بزرگ/کوچک می‌شود.
4. در **هر دو** نسخه هست: هم نصب‌شده و هم `target/release`.

⇒ این **باقی‌ماندهٔ درخشش اورب در موقعیت/اندازهٔ قبلی** است، نه کمانِ ناحیهٔ کلیک.

### ۱۶٫۳ چه چیزهایی **رد شدند** (با آزمایش، نه استدلال)

| فرضیه | آزمایش | نتیجه |
|---|---|---|
| نسخهٔ نصب‌شده قدیمی‌تر است و اصلاح را ندارد | `GetWindowRgn` روی هر دو | **رد شد** — هر دو `76,76 146x146` |
| ریفکتورِ `overlay` هاله را برگردانده | `git diff --stat` | **رد شد** — `orb.rs` و `window_shape.rs` صفر خط تغییر |
| زمان ساخت ۱۳:۴۹ قبل از کامیت ۱۳:۵۱ ⇒ بیلد قدیمی | هش + ناحیه | **رد شد** — از همان working tree بوده |
| `force_repaint` سطح بازترسیم را پاک می‌کند | — | **بی‌اثر** — DWM آن را دور نمی‌اندازد |

### ۱۶٫۴ باطل‌شدنِ قضاوت قبلی

پروب `orb-ghost-check.ps1` پنج بار (و بعداً ۳۳۱ بار) `clean` داد و از روی آن گفته شد هاله حل شده.
**آن نتیجه بی‌معنا بود.** پروب یک خط عمودی از مرکز *فعلی* اورب می‌گیرد:

| حالت | شعاع ناحیه | رأس کمان | نسبت به اورب |
|---|---|---|---|
| Recording | ۱۲۰٫۳px | y=۲۳۱ | بیرون ⇒ دیده می‌شود |
| Idle | ۶۹٫۸px | y=۲۸۲ | زیر لبهٔ اورب (۲۶۶) ⇒ **پنهان** |

همهٔ ۳۳۱ نمونه در Idle گرفته شد ⇒ پروب **ساختاراً کور** بود. ابزار سالم، نتیجهٔ بی‌معنا.

### ۱۶٫۵ شاهدِ مثبت

`voice-ptt/target/voice-ptt-prev-193a523.exe` هالهٔ **قدیمیِ «نوار بالای اورب»** را نشان می‌دهد.
برای هر آنالایزرِ جدیدی که نوشته می‌شود باید اول روی این باینری کالیبره شود — وگرنه
«ندیدم» با «ندارد» تفاوتی ندارد.

### ۱۶٫۶ نتیجهٔ راهبردی

پنجره **یک‌بار** در بزرگ‌ترین حالت ممکن ساخته می‌شود و هرگز resize نمی‌شود. پس آرتیفکت جابه‌جایی
از پنجره نمی‌آید؛ از **محتوای رندرشده‌ای** می‌آید که DWM در سطح بازترسیم نگه داشته. درمان یعنی
وادار کردن DWM به دور انداختن آن سطح — و این در Win32 راه تمیزی ندارد.

گزینه‌های بررسی‌نشده: `DwmFlush()` بعد از هر جابه‌جایی · `WS_EX_NOREDIRECTIONBITMAP` ·
رندر کردن اورب در پنجرهٔ **جداگانه**.

> **نکتهٔ روشی:** این سه گزینه هنوز آزموده نشده‌اند. قبل از اجرا، بخش «کوری پروب‌ها» در
> [LESSONS-LEARNED.md](LESSONS-LEARNED.md) را بخوانید و پروب را طوری بسازید که در حالت
> Idle هم بتواند آرتیفکت را ببیند.

---

## به‌روزرسانی ۲۰۲۶-۰۹-۳۰ — پنجره کوچک‌تر شد، کمان **تأییدنشده** ماند

بوم پنجرهٔ میزبان از `۲۳۸` به `۲۳۶٫۸۶` واحد رفت (اندازه‌گیری‌شده روی نمایشگر:
۲۹۸×۲۹۸ px ⇒ **۲۹۶×۲۹۶ px**، `dpi=120`). عدد قبلی از قبل درست بود، به‌طور
تصادفی — از جمع `۱٫۹۰ × قطر` (که به‌تنهایی کم بود) و ۴۸ واحد بالشتک. حالا از
هندسهٔ واقعی رسم مشتق می‌شود. جزئیات کامل: [MEASURED-FACTS.md](MEASURED-FACTS.md) بند ۱۵.

**آنچه واقعاً تغییر کرد:** مرکز اُرب دیگر یک نیم‌بوم از لبه فاصله ندارد. قبلاً
۱۱۹ واحد، حالا ۶۶٫۹ واحد در Idle (لبهٔ رسم‌شده + ۶ واحد حاشیه) — و روی هر چهار
لبهٔ ناحیهٔ کاری **۹۰ px = ۷۱٫۶ واحد** اندازه‌گیری شد. ضمناً محدودیت کشیدن حالا
ناحیهٔ کاری نمایشگر را مبنا می‌گیرد، نه کل دسکتاپ مجازی، پس اُرب دیگر زیر نوار
وظیفه کشیده نمی‌شود.

### کمان سفید: چرا هنوز «تأییدنشده» است

> **این بند تاریخچهٔ همان روز است.** دو تا از سه کمبودی که اینجا فهرست شده بعداً
> ساخته شدند: کنترل مثبت ([halo-selftest.py](reaserch/gui/probes/halo-selftest.py)) و
> کشیدنِ بلند با تأیید هندسی ([halo-hunt.ps1](reaserch/gui/probes/halo-hunt.ps1)).
> وضعیت نهایی در «به‌روزرسانی ۲۰۲۶-۱۰-۰۱» پایین همین سند است.

سه چیز را باید کنار هم گفت:

۱. **بازتولید نشد.** نه در ساخت قدیمی، نه در ساخت جدید، در هیچ اجرایی. نه با
   `orb-drag-probe.ps1` و نه با پروب تازهٔ هندسه.

۲. **اولین تحلیل‌گر کور بود.** تطبیق رنگ اندازه‌گیری‌شدهٔ کمان
   (`(215,229,243)`) در یک اسکرین‌شات معمولی از دسکتاپ روشن **۲۱٬۶۹۳ پیکسل**
   پیدا کرد — یعنی داشت *خود برنامه* را اندازه می‌گرفت، نه آرتیفکت را. این سومین
   نمونهٔ یک درس واحد در این پروژه است: **ابزار سالم ≠ نتیجهٔ معتبر** (نمونه‌های
   قبلی: `orb-ghost-check.ps1` و هارنس جهش فاز ۳).

۳. **کنترل مثبت وجود ندارد.** تحلیل‌گر دوم حساس است (۳٬۳۳۶ پیکسل روشن تازه را
   در مستطیل قدیمی می‌بیند) ولی توالی کشیدنی که خودکار می‌شود طوری تمام می‌شود
   که اُرب روی مستطیل قبلی خودش هم‌پوشانی دارد، و آن‌وقت «پیکسل روشن در مستطیل
   قدیمی» صرفاً بدنهٔ خود اُرب است. برازش دایره روی آن داده‌ها باقی‌ماندهٔ ۹۰
   درصدی ۳۷٫۳ px داد — یعنی توده، نه کمان نازک ۱۰ پیکسلی.

بنابراین: **نه رفع شده، نه بازتولید شده.** کوچک‌شدن پنجره و سبزی تست‌های
محاسباتی دلیل رفع شدن نیستند، و این گزارش هم ادعای دیگری نمی‌کند.

ابزارها، با محدودیتشان ثبت‌شده:
[orb-geometry-probe.ps1](reaserch/gui/probes/orb-geometry-probe.ps1) ·
[arc-analyse.py](reaserch/gui/probes/arc-analyse.py)

### آنچه برای اثبات لازم است

۱. ~~یک کشیدنِ بلند و پیوسته که اُرب را از یک سمت صفحه به سمت دیگر ببرد تا مستطیل
   ترک‌شده واقعاً خالی باشد (چهار کشیدن کوتاه لبه‌ای کافی نیست).~~ **انجام شد** — ۱۸۵۰ پیکسل،
   با شرط تأیید هندسیِ مستقل از تحلیل (۲۹۳ پیکسل لازم).
۲. ~~**یک کنترل مثبت**~~ **انجام شد** — پنج کنترل در `halo-selftest.py` که دروازهٔ
   اجرای شکارند؛ اگر یکی اختلاف پیدا کند، شکار اصلاً اجرا نمی‌شود.
۳. حذف/کوچک‌کردن پنجره‌های پوشانندهٔ دسکتاپ پیش از خواندن `WindowFromPoint`،
   چون آن خوانش در وضعیت فعلی دسکتاپ پاسخِ Chrome را می‌دهد و چیزی دربارهٔ
   ناحیهٔ اُرب نمی‌گوید. **هنوز باز است** — ولی دیگر مسیر بحرانی نیست، چون شکار
   اکنون با انتخاب پنجره از مرکز `config.toml` کار می‌کند، نه با `WindowFromPoint`.
---

## به‌روزرسانی ۲۰۲۶-۱۰-۰۱ — شکار خودکار: `NOT REPRODUCED`، شش بار پیاپی

بخش قبلی دو چیز را به‌عنوان «کمبود» ثبت کرده بود: نبودِ **کنترل مثبت**، و ناتوانی از
کشیدنِ بلند. هر دو ساخته شدند و نتیجه این است:

```
SELFTEST PASSED           (هر اجرا، پیش از هر اندازه‌گیری)
-- old rect: 1665,109 296x296px  centre=(1813,257)
-- drag attempt 1 ... the orb moved 1850px
-- vacate check: moved 1850px, needs > 293px
-- new bright pixels inside the OLD rect: 0
VERDICT: NOT REPRODUCED
```

**۱) کنترل مثبت حالا وجود دارد** — و دروازهٔ شکار است. [halo-selftest.py](reaserch/gui/probes/halo-selftest.py)
پنج جفت اسکرین‌شات مصنوعی با اعداد بند ۱۶٫۱ می‌سازد و **تحلیل‌گر واقعی** را روی آن‌ها
اجرا می‌کند: کمان واقعی (۳٬۶۴۸ پیکسل) ⇒ `REPRODUCED` با شعاع برازشی ۱۱۶٫۲ در برابر
۱۲۰٫۴ رسم‌شده · صفحهٔ بی‌تغییر ⇒ `CLEAN` · بدنهٔ اورب ۱۸٬۱۲۵ · اورب نیمه‌خالی ۱۴٬۶۴۹ ·
رابط کاربری روشن با **رنگ دقیق هاله، ۵۱٬۶۰۰ پیکسل** ⇒ هر سه `NOT_AN_ARC`.
سطر آخر همان تلهٔ بخش قبلی است که این‌بار **کنترل** شد: آن نسخه ۲۱٬۶۹۳ پیکسل را
«کمان» نامید؛ این نسخه ۵۱٬۶۰۰ پیکسل از همان جنس می‌بیند و رد می‌کند.

**۲) کشیدن بلند انجام شد** — ۱۸۵۰ پیکسل، و این‌بار با **تأیید هندسی پیش از تحلیل**:
فاصلهٔ مستقیم مرکز قدیم تا جدید باید از (گوشهٔ مستطیل ۲۰۹ + شعاع نقاشی اورب ۷۶ +
حاشیه ۸) = ۲۹۳ پیکسل بیشتر باشد، وگرنه حکم `INCONCLUSIVE` است حتی اگر تحلیل‌گر
چیزی بدهد. شش اجرای پیاپی: صفر پیکسل روشن تازه در مستطیل ترک‌شده، هر بار.

### دو نقص واقعی که همین کار پیدا کرد

- **پروب در فضای مختصات اشتباه بود.** نسخهٔ نخست [halo-hunt.ps1](reaserch/gui/probes/halo-hunt.ps1)
  `SetProcessDpiAwarenessContext` را صدا نمی‌زد ⇒ `GetWindowRect` مختصات را ÷۱٫۲۵
  می‌داد (۲۹۶×۲۹۶ → ۲۳۷×۲۳۷ و مرکز ۱۸۱۳ → ۱۴۵۰٫۵) در حالی که `CopyFromScreen`
  همچنان پیکسل فیزیکی می‌گرفت. بررسیِ «کشیدن مستطیل را خالی کرد» از اعدادی
  محاسبه می‌شد که ۲۵٪ کوچک‌تر بودند — همان تلهٔ بند ۱۵، این‌بار در ابزار تازه.
  [list-windows.ps1](reaserch/gui/probes/list-windows.ps1) ابزاری بود که این را آشکار کرد.
- **«اولین پنجرهٔ مربعی» اورب نیست.** حالا انتخاب بر پایهٔ مرکزِ `config.toml` (±۴۰px)
  است و اگر پیدا نشود پروب **رد می‌کند** به‌جای اینکه چیزی را تحلیل کند.

### آنچه هنوز ثابت نشده

کوچک‌شدن ناحیه با **تغییر حالت** (بازگشت اورب از Recording به Idle) آزمایش نشد —
برای آن دیکتهٔ واقعی و میکروفون لازم است. پس‌زمینهٔ روشن هم آزمایش نشد. و
**«پیدا نشد» ≠ «رفع شد»**: فقط یعنی در این مسیر، در این بیلد، شش بار بازتولید نشد.

جزئیات کامل: [MEASURED-FACTS.md](MEASURED-FACTS.md) بند ۲۰.