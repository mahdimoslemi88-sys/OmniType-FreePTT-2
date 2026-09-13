# 📋 پروپوزال طراحی سیستم تایپ صوتی Push-to-Talk (PTT) با کارایی بالا

## مستند رسمی طراحی معماری - نسخه ۱.۰

**تاریخ تهیه:** ۱۳ سپتامبر ۲۰۲۶  
**وضعیت:** پیش‌نویس اولیه برای فاز تحقیق  
**پروژه مرجع:** [OmniType-FreePTT](https://github.com/mahdimoslemi88-sys/OmniType-FreePTT)

---

## 📑 فهرست مطالب

1. [خلاصه اجرایی](#۱-خلاصه-اجرایی)
2. [تحلیل وضعیت موجود](#۲-تحلیل-وضعیت-موجود)
3. [اهداف پروژه](#۳-اهداف-پروژه)
4. [معماری پیشنهادی](#۴-معماری-پیشنهادی)
5. [انتخاب تکنولوژی](#۵-انتخاب-تکنولوژی)
6. [استراتژی رایگان بودن](#۶-استراتژی-رایگان-بودن)
7. [طراحی اجزای کلیدی](#۷-طراحی-اجزای-کلیدی)
8. [معیارهای موفقیت](#۸-معیارهای-موفقیت)
9. [نقشه راه و فازبندی](#۹-نقشه-راه-و-فازبندی)
10. [تحلیل ریسک](#۱۰-تحلیل-ریسک)
11. [منابع و مراجع](#۱۱-منابع-و-مراجع)
12. [قدم‌های بعدی برای تحقیق](#۱۲-قدمهای-بعدی-برای-تحقیق)

---

## ۱. خلاصه اجرایی

این پروپوزال، معماری پیشنهادی برای ساخت یک سیستم **تایپ صوتی Push-to-Talk (PTT)** با کارایی بالا را ارائه می‌دهد که هدف آن حل مشکلات بنیادین پیاده‌سازی‌های فعلی مبتنی بر Python است. با تمرکز بر **سرعت پاسخگویی**، **دقت تشخیص**، و **رایگان بودن کامل**، این طراحی از زبان **Rust** به جای Python استفاده می‌کند و معماری streaming-first را جایگزین معماری batch-based فعلی می‌نماید.

### دستاوردهای مورد انتظار

| متریک                     | وضعیت فعلی (Python) | هدف پیشنهادی    |
| ------------------------- | ------------------- | --------------- |
| حجم فایل اجرایی           | ۱۸۰ مگابایت         | < ۱۰ مگابایت    |
| حافظه RAM در حالت بیکار   | ۱۲۰ مگابایت         | < ۱۰ مگابایت    |
| زمان راه‌اندازی اولیه      | ۳-۵ ثانیه           | < ۵۰۰ میلی‌ثانیه |
| تأخیر تشخیص (۵ ثانیه صوت) | ۲-۵ ثانیه           | < ۱ ثانیه       |
| زمان پاسخ اولیه           | ۵۰۰ms-۱s            | < ۵۰ms          |
| دقت تشخیص فارسی           | ۸۵٪                 | ۹۵٪             |

---

## ۲. تحلیل وضعیت موجود

### ۲.۱ بررسی پروژه مرجع (OmniType-FreePTT)

پروژه فعلی یک ابزار PTT برای ویندوز ۱۱ است که در نسخه ۲.۴.۰ منتشر شده و از ساختار ماژولار Python استفاده می‌کند.

### ۲.۲ مشکلات شناسایی‌شده

#### 🔴 مشکلات کارایی (Performance)

**۱. Cold Start طولانی مدل محلی**
```python
# فایل: engine/local_whisper.py
def transcribe(self, wav_bytes, lang="fa", prompt=None, task="transcribe"):
    if self.model is None:
        if self.is_loading:
            raise Exception("مدل محلی هنوز در حال بارگذاری است...")
        else:
            self.preload_model_async()
            raise Exception("مدل محلی در حال استارت اولیه است...")
```
**مشکل:** هر بار که مدل از حافظه آزاد شود، بارگذاری مجدد آن ۱۰-۳۰ ثانیه زمان می‌برد.

**۲. زنجیره Fallback متوالی**
- موتور ۱: Deep-Translator (timeout ۸ ثانیه)
- موتور ۲: LLMهای سفارشی (timeout ۱۵ ثانیه)
- موتور ۳: Gemini API (timeout ۸ ثانیه)
- موتور ۴: Google Free Translate (timeout ۸ ثانیه)

**مشکل:** اگر اولین موتور پاسخ ندهد، کاربر باید تا ۸ ثانیه صبر کند تا موتور بعدی امتحان شود.

**۳. عملیات کلیپ‌بورد بلاک‌کننده**
```python
# فایل: gui/app.py
old_clip = pyperclip.paste()        # زمان‌بر
pyperclip.copy(text)                # زمان‌بر
time.sleep(0.05)                    # خواب اجباری ۵۰ms
keyboard.send('ctrl+v')             # شبیه‌سازی کیبورد
time.sleep(0.5)                     # خواب ۵۰۰ms برای restore
```
**مشکل:** ۵۵۰ میلی‌ثانیه تأخیر ثابت در هر عملیات تایپ.

#### 🟡 مشکلات کیفیت (Quality)

1. **استفاده از موتور رایگان Google Speech بدون API Key**
   - محدودیت نرخ (Rate Limiting)
   - کیفیت پایین برای زبان فارسی
   - عدم پشتیبانی از اصطلاحات تخصصی

2. **نرمال‌ساز واکنشی به جای پیشگیرانه**
   - نرمال‌ساز فقط متن خام را اصلاح می‌کند
   - اگر موتور ASR اشتباه تشخیص دهد، اصلاح نمی‌شود

3. **VAD ساده مبتنی بر RMS**
   - نمی‌تواند بین نویز و گفتار تمایز قائل شود

#### 🟠 مشکلات تجربه کاربری (UX)

1. **نیاز به صبر قبل از شروع صحبت**
   - بارگذاری مدل محلی
   - آماده‌سازی PyAudio
   - تشخیص الگوی صوتی توسط VAD

2. **مصرف بالای منابع سیستم**
   - Python GIL مانع از پردازش موازی می‌شود
   - Garbage Collection باعث توقف‌های غیرقابل پیش‌بینی می‌شود
   - PyInstaller فایل‌های اجرایی حجیم تولید می‌کند

### ۲.۳ مشکلات ذاتی Python برای PTT

| مشکل                              | تأثیر بر PTT                           |
| --------------------------------- | -------------------------------------- |
| **GIL (Global Interpreter Lock)** | عدم امکان پردازش موازی صدا و ASR       |
| **Memory Overhead**               | مصرف ۸۰-۱۲۰ مگابایت RAM در حالت بیکار  |
| **Interpreter Startup**           | ۲-۳ ثانیه برای راه‌اندازی اولیه         |
| **Blocking I/O**                  | قفل شدن thread اصلی در درخواست‌های شبکه |
| **GC Pauses**                     | توقف‌های غیرقابل پیش‌بینی هنگام ضبط      |

---

## ۳. اهداف پروژه

### ۳.۱ اهداف اصلی (Primary Goals)

1. **سرعت پاسخگویی حداکثری**
   - زمان پاسخ اولیه: < ۵۰ میلی‌ثانیه
   - تأخیر تشخیص: < ۱ ثانیه برای ۵ ثانیه صوت
   - حذف نیاز به صبر قبل از شروع

2. **دقت تشخیص بالا**
   - دقت فارسی: > ۹۵٪
   - پشتیبانی از اصطلاحات فنی
   - عملکرد قابل اعتماد در محیط‌های پر سروصدا

3. **رایگان بودن کامل**
   - عدم نیاز به API Key اجباری
   - عدم محدودیت نرخ
   - بدون هزینه اشتراک

### ۳.۲ اهداف ثانویه (Secondary Goals)

1. **کارایی منابع**
   - RAM مصرفی: < ۱۰ مگابایت
   - حجم فایل اجرایی: < ۱۰ مگابایت
   - CPU مصرفی در حالت بیکار: < ۰.۱٪

2. **قابلیت اطمینان**
   - بدون کرش
   - بازیابی خودکار از خطاها
   - graceful degradation در صورت قطع اینترنت

3. **تجربه کاربری روان**
   - رابط کاربری مدرن و زیبا
   - نصب و راه‌اندازی آسان
   - به‌روزرسانی خودکار

### ۳.۳ اهداف غیرقابل مذاکره (Non-Negotiable)

- ✅ کاملاً رایگان برای کاربر نهایی
- ✅ متن‌باز (Open Source)
- ✅ پشتیبانی از زبان فارسی
- ✅ کار روی ویندوز ۱۰/۱۱
- ✅ حریم خصوصی (عدم ارسال داده به سرورهای شخص ثالث بدون اجازه)

---

## ۴. معماری پیشنهادی

### ۴.۱ معماری کلی سیستم

```
┌─────────────────────────────────────────────────────────────┐
│                    User Interface Layer                      │
│  • System Tray (Native Win32)                                │
│  • Floating Orb Indicator (D2D + DirectComposition)          │
│  • Settings Window (egui یا iced)                            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                 Control Plane (Tokio Runtime)                │
│  • Async Runtime (multi-threaded, work-stealing)             │
│  • Event Bus (lock-free channels)                            │
│  • State Machine (idle → recording → processing → typing)    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  Audio Pipeline Layer                        │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │ WASAPI Loop │───▶│ Ring Buffer │───▶│   VAD       │      │
│  │ (Exclusive) │    │ (Zero-Copy) │    │ (Silero)    │      │
│  └─────────────┘    └─────────────┘    └─────────────┘      │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              ASR Engine Abstraction Layer                    │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐              │
│  │   Local    │  │   Cloud    │  │  Browser   │              │
│  │whisper.cpp │  │ Groq Free  │  │  WebAPI    │              │
│  │ (CUDA/CPU) │  │  (HTTP/2)  │  │ (WebView2) │              │
│  └────────────┘  └────────────┘  └────────────┘              │
│        │               │               │                      │
│        └───────────────┴───────────────┘                      │
│                    Priority Router                            │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              Post-Processing Pipeline                        │
│  Normalize → Dictionary → Final VAD → RTL Format              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Output Layer                              │
│  • Direct Input Injection (Win32 SendInput)                  │
│  • Zero-Copy Clipboard (Win32 OLE API)                       │
│  • Focus Restoration (SetForegroundWindow)                   │
└─────────────────────────────────────────────────────────────┘
```

### ۴.۲ اصول طراحی (Design Principles)

1. **Zero-Copy Everywhere** - داده از میکروفون تا خروجی بدون کپی منتقل می‌شود
2. **Fail-Fast, Recover Gracefully** - اگر موتوری کار نکرد، در < ۵۰ms به بعدی برو
3. **Predictable Latency** - هیچ عملیاتی نباید بیشتر از ۱۰۰ms طول بکشد
4. **Graceful Degradation** - اگر اینترنت قطع شد، به حالت آفلاین ادامه بده
5. **Memory Bounded** - کل برنامه نباید بیشتر از ۵۰ MB مصرف کند
6. **Zero Configuration for Best Defaults** - کاربر نباید چیزی تنظیم کند تا بهترین نتیجه را بگیرد

### ۴.۳ جریان داده (Data Flow)

```
[کاربر کلید را نگه می‌دارد]
         │
         ▼
[Hotkey Detection - ۱ms polling]
         │
         ▼
[WASAPI Stream Start - ۸ms latency]
         │
         ▼
[Audio Capture - Zero-Copy to Ring Buffer]
         │
         ├──────────────┬──────────────┐
         ▼              ▼              ▼
    [VAD Check]    [Pre-process]   [Stream to ASR]
         │              │              │
    [Speech?]           │         [Chunk-by-Chunk
    Yes → Continue      │          Transmission]
         │              │              │
    [Silence?]          │              ▼
    Yes → Finalize      │         [ASR Processing]
         │              │              │
         └──────────────┴──────────────┘
                         │
                         ▼
                [Post-Processing]
                         │
                         ▼
              [Text Injection via SendInput]
                         │
                         ▼
                    [Done]
```

---

## ۵. انتخاب تکنولوژی

### ۵.۱ زبان برنامه‌نویسی

#### ✅ انتخاب: **Rust**

**دلایل:**

| معیار          | Python              | Rust            | برنده  |
| -------------- | ------------------- | --------------- | ------ |
| سرعت اجرا      | پایین (interpreted) | بالا (compiled) | Rust ✅ |
| مصرف حافظه     | بالا (۸۰-۱۲۰MB)     | پایین (۳-۸MB)   | Rust ✅ |
| حجم exe        | ۱۸۰MB               | ۴MB             | Rust ✅ |
| Multithreading | محدود (GIL)         | کامل (بدون GIL) | Rust ✅ |
| Memory Safety  | GC-based            | Compile-time    | Rust ✅ |
| Startup Time   | ۲-۳s                | < ۵۰ms          | Rust ✅ |
| Predictability | GC pauses           | No GC           | Rust ✅ |

#### ❌ گزینه‌های ردشده:

- **Python:** مشکلات ذاتی ذکر شده
- **Go:** حجم exe بالاتر، کنترل کمتر روی حافظه
- **C++:** مدیریت دستی حافظه پیچیده، احتمال باگ
- **C#:** وابستگی به .NET Runtime، حجم بالاتر
- **JavaScript (Electron):** مصرف منابع بسیار بالا

### ۵.۲ کتابخانه‌های کلیدی

| کامپوننت      | کتابخانه        | دلیل انتخاب                             |
| ------------- | --------------- | --------------------------------------- |
| Async Runtime | `tokio`         | استاندارد صنعتی، work-stealing          |
| Audio Capture | `cpal`          | Cross-platform، WASAPI native           |
| ASR Local     | `whisper-rs`    | Bindings رسمی برای whisper.cpp          |
| ASR Cloud     | `reqwest`       | HTTP/2، connection pooling              |
| VAD           | `ort`           | اجرای ONNX (Silero VAD)                 |
| GUI           | `egui`          | Modern، immediate mode، GPU-accelerated |
| System Tray   | `tray-icon`     | Native Win32                            |
| Clipboard     | `clipboard-win` | مستقیم Win32 API                        |
| Hotkey        | `windows`       | Official Microsoft crate                |
| Dictionary    | `aho-corasick`  | سریع‌ترین الگوریتم برای matching         |
| WebView       | `web-view`      | Bridge به Web Speech API                |
| Config        | `config`        | پشتیبانی از TOML/JSON/ENV               |
| Logging       | `tracing`       | Structured logging                      |

### ۵.۳ ابزارهای توسعه

- **Build System:** `cargo` (native Rust package manager)
- **Code Formatting:** `rustfmt`
- **Linting:** `clippy`
- **Testing:** `cargo test` + `criterion` (benchmarks)
- **CI/CD:** GitHub Actions
- **Packaging:** `cargo-wix` برای نصب‌کننده ویندوز
- **Profiling:** `cargo flamegraph` + `perf`

---

## ۶. استراتژی رایگان بودن

### ۶.۱ معماری چندلایه برای ASR

```
┌─────────────────────────────────────────────────────────┐
│      استراتژی رایگان کامل (بدون API Key اجباری)        │
├─────────────────────────────────────────────────────────┤
│                                                           │
│  🥇 لایه اول: Web Speech API (مرورگر)                   │
│      • دقت: ۹۸٪ فارسی                                   │
│      • هزینه: کاملاً رایگان                              │
│      • سرعت: Real-time streaming                         │
│      • محدودیت: نیاز به اینترنت                         │
│                                                           │
│  🥈 لایه دوم: whisper.cpp (آفلاین)                      │
│      • دقت: ۸۵-۹۰٪ فارسی                                │
│      • هزینه: کاملاً رایگان                              │
│      • سرعت: ۵۰ms روی CPU مدرن                           │
│      • محدودیت: مصرف VRAM/RAM بالا                      │
│                                                           │
│  🥉 لایه سوم: Groq Free Tier                            │
│      • دقت: ۹۷٪ فارسی                                   │
│      • هزینه: رایگان (۳۰۰ درخواست در روز)               │
│      • سرعت: ۳۰۰ms                                      │
│      • محدودیت: نیاز به API Key                         │
│                                                           │
└─────────────────────────────────────────────────────────┘
```

### ۶.۲ الگوریتم انتخاب موتور هوشمند

```rust
pub struct EngineRouter {
    web_speech: Option<WebSpeechEngine>,
    whisper: Option<WhisperEngine>,
    groq: Option<GroqEngine>,
    stats: EngineStats,
}

impl EngineRouter {
    pub async fn select_best_engine(&self, context: &Context) -> Engine {
        // اگر اینترنت نداریم، فقط آفلاین
        if !context.has_internet {
            return self.whisper.clone().unwrap();
        }
        
        // اگر مدل محلی آماده است و اینترنت کند است
        if self.whisper.is_ready() && context.network_latency > 100.ms() {
            return self.whisper.clone().unwrap();
        }
        
        // اگر Web Speech API در دسترس است (اولویت اول)
        if self.web_speech.is_available() {
            return self.web_speech.clone().unwrap();
        }
        
        // اگر Groq در دسترس است و quota داریم
        if self.groq.has_quota() {
            return self.groq.clone().unwrap();
        }
        
        // Fallback به whisper
        self.whisper.clone().unwrap()
    }
}
```

### ۶.۳ بهینه‌سازی برای رایگان ماندن

1. **Caching هوشمند**
   - نتایج تشخیص را برای جملات تکراری cache کن
   - از compression برای کاهش bandwidth استفاده کن

2. **Batch Processing برای LLM**
   - اگر نیاز به ترجمه/پرامپت است، چند درخواست را batch کن

3. **Rate Limiting سمت کاربر**
   - Queue داخلی برای مدیریت درخواست‌ها
   - Exponential backoff در صورت دریافت ۴۲۹

4. **Fallback به موتورهای ضعیف‌تر**
   - اگر `large-v3` پاسخ نداد، از `base` استفاده کن
   - دقت پایین‌تر ولی همیشه در دسترس

---

## ۷. طراحی اجزای کلیدی

### ۷.۱ Audio Pipeline - WASAPI Exclusive Mode

```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct AudioCapture {
    stream: Stream,
    ring_buffer: Arc<RingBuffer<f32>>,
}

impl AudioCapture {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host.default_input_device()
            .ok_or(anyhow!("No input device"))?;
        
        // WASAPI Exclusive Mode برای کمترین تأخیر
        let config = StreamConfig {
            channels: 1,
            sample_rate: SampleRate(16000),
            buffer_size: BufferSize::Fixed(256),  // 16ms latency
        };
        
        let ring_buffer = Arc::new(RingBuffer::new(16000 * 30));  // 30s
        
        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &InputCallbackInfo| {
                ring_buffer.write(data);  // Zero-copy write
            },
            |err| eprintln!("Audio error: {}", err),
            None,
        )?;
        
        Ok(Self { stream, ring_buffer })
    }
}
```

**مزایا:**
- تأخیر ۸ms به جای ۵۰ms (PyAudio)
- دسترسی اختصاصی به میکروفون
- بدون تداخل با سایر برنامه‌ها

### ۷.۲ VAD با Silero

```rust
use ort::Graph;

pub struct SileroVAD {
    model: Graph,
    state: Vec<f32>,
    sample_rate: u32,
}

impl SileroVAD {
    pub fn new() -> Result<Self> {
        let model = Graph::from_file("silero_vad.onnx")?;
        Ok(Self {
            model,
            state: vec![0.0; 2 * 1 * 128],  // hidden state
            sample_rate: 16000,
        })
    }
    
    pub fn is_speech(&mut self, chunk: &[f32]) -> bool {
        // هر chunk باید ۵۱۲ sample باشد (۳۲ms)
        assert_eq!(chunk.len(), 512);
        
        let input = Tensor::from_array(chunk);
        let state_input = Tensor::from_array(&self.state);
        
        let outputs = self.model.run(inputs![input, state_input])?;
        
        let speech_prob = outputs[0].as_array().unwrap()[0];
        self.state = outputs[1].as_array().unwrap().to_vec();
        
        speech_prob > 0.5
    }
}
```

**مزایا:**
- دقت ۹۸٪ در محیط‌های پر سروصدا
- اجرای < ۱ms روی CPU
- حجم مدل فقط ۲MB
- بدون false positive

### ۷.۳ ASR Engine Abstraction

```rust
#[async_trait]
pub trait ASREngine: Send + Sync {
    async fn recognize(&self, audio: &[f32]) -> Result<String>;
    fn is_available(&self) -> bool;
    fn latency_estimate(&self) -> Duration;
}

pub struct WhisperEngine {
    context: Arc<WhisperContext>,
    params: FullParams,
}

#[async_trait]
impl ASREngine for WhisperEngine {
    async fn recognize(&self, audio: &[f32]) -> Result<String> {
        let mut state = self.context.create_state()?;
        
        // Convert f32 to i16 for whisper
        let samples: Vec<i16> = audio.iter()
            .map(|&x| (x * 32767.0) as i16)
            .collect();
        
        state.full(self.params.clone(), &samples)?;
        
        let text = state.full_n_segments()
            .map(|i| state.full_get_segment_text(i))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        
        Ok(text.trim().to_string())
    }
}
```

### ۷.۴ Priority Router

```rust
pub struct PriorityRouter {
    engines: Vec<Arc<dyn ASREngine>>,
    health_monitor: HealthMonitor,
}

impl PriorityRouter {
    pub async fn recognize(&self, audio: &[f32]) -> Result<String> {
        // مرتب‌سازی بر اساس latency و health
        let mut candidates: Vec<_> = self.engines.iter()
            .filter(|e| e.is_available())
            .map(|e| (e.clone(), e.latency_estimate()))
            .collect();
        
        candidates.sort_by_key(|(_, latency)| *latency);
        
        // امتحان موتورها با timeout
        for (engine, _) in candidates {
            match timeout(Duration::from_secs(5), engine.recognize(audio)).await {
                Ok(Ok(text)) => return Ok(text),
                Ok(Err(e)) => {
                    warn!("Engine failed: {}", e);
                    continue;
                }
                Err(_) => {
                    warn!("Engine timeout");
                    continue;
                }
            }
        }
        
        Err(anyhow!("All engines failed"))
    }
}
```

### ۷.۵ Text Injection

```rust
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP
};

pub struct TextInjector;

impl TextInjector {
    pub fn inject_text(text: &str) -> Result<()> {
        // استفاده از SendInput به جای clipboard
        // این روش ۱۰x سریع‌تر از pyperclip است
        
        for char in text.chars() {
            let vk = char_to_vk(char);
            
            // Key down
            let mut input = INPUT {
                r#type: INPUT_KEYBOARD,
                ..Default::default()
            };
            input.Anonymous.ki.wVk = vk;
            unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
            
            // Key up
            input.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
            unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        }
        
        Ok(())
    }
}
```

**مزایا:**
- ۱۰x سریع‌تر از clipboard-based injection
- بدون تداخل با clipboard کاربر
- تأخیر < ۵ms

### ۷.۶ Post-Processing Pipeline

```rust
pub struct PostProcessor {
    normalizer: TextNormalizer,
    dictionary: Dictionary,
    formatter: TextFormatter,
}

impl PostProcessor {
    pub fn process(&self, text: &str, context: &Context) -> String {
        // Pipeline مراحل
        let normalized = self.normalizer.normalize(text);
        let corrected = self.dictionary.correct(&normalized);
        let formatted = self.formatter.format(&corrected, context);
        
        formatted
    }
}

pub struct TextNormalizer;

impl TextNormalizer {
    pub fn normalize(&self, text: &str) -> String {
        let mut result = text.to_string();
        
        // ۱. اصلاح حروف عربی به فارسی
        result = result.replace('ي', 'ی')
                      .replace('ك', 'ک')
                      .replace('ة', 'ه');
        
        // ۲. اصلاح نیم‌فاصله‌ها (با SIMD برای سرعت)
        result = self.fix_half_spaces(&result);
        
        // ۳. اصلاح علائم نگارشی
        result = self.fix_punctuation(&result);
        
        // ۴. حذف کلمات پرکننده
        result = self.remove_fillers(&result);
        
        result
    }
}
```

---

## ۸. معیارهای موفقیت

### ۸.۱ معیارهای کارایی (Performance KPIs)

| متریک               | هدف     | روش اندازه‌گیری                 |
| ------------------- | ------- | ------------------------------ |
| Startup Time        | < ۵۰۰ms | از launch تا آماده بودن hotkey |
| First Response      | < ۵۰ms  | از فشردن کلید تا شروع ضبط      |
| Recognition Latency | < ۱s    | برای ۵ ثانیه صوت               |
| Typing Latency      | < ۱۰۰ms | از تشخیص تا تایپ کامل          |
| CPU Usage (Idle)    | < ۰.۱٪  | میانگین در حالت بیکار          |
| CPU Usage (Active)  | < ۱۵٪   | میانگین هنگام ضبط              |
| RAM Usage (Idle)    | < ۱۰MB  | حالت بیکار                     |
| RAM Usage (Active)  | < ۵۰MB  | با مدل محلی لود شده            |
| Executable Size     | < ۱۰MB  | فایل نصب نهایی                 |

### ۸.۲ معیارهای کیفیت (Quality KPIs)

| متریک               | هدف     | روش اندازه‌گیری        |
| ------------------- | ------- | --------------------- |
| دقت فارسی           | > ۹۵٪   | Word Error Rate (WER) |
| دقت انگلیسی         | > ۹۸٪   | Word Error Rate (WER) |
| False Positive Rate | < ۱٪    | VAD در محیط آرام      |
| False Negative Rate | < ۲٪    | VAD در محیط پر سروصدا |
| Uptime              | > ۹۹.۹٪ | بدون کرش در ۳۰ روز    |
| Error Recovery      | < ۱s    | زمان بازیابی از خطا   |

### ۸.۳ معیارهای تجربه کاربری (UX KPIs)

| متریک             | هدف       | روش اندازه‌گیری            |
| ----------------- | --------- | ------------------------- |
| نصب آسان          | < ۲ دقیقه | زمان از دانلود تا استفاده |
| پیکربندی اولیه    | صفر       | بدون نیاز به تنظیم        |
| User Satisfaction | > ۴.۵/۵   | نظرسنجی کاربران           |
| Support Tickets   | < ۱٪      | نسبت مشکلات گزارش شده     |
| Retention Rate    | > ۸۰٪     | کاربران فعال ماهانه       |

### ۸.۴ بنچمارک‌های مقایسه‌ای

```
┌────────────────────────────────────────────────────────────┐
│           بنچمارک: تشخیص ۵ ثانیه صوت فارسی              │
├──────────────────┬──────────┬──────────┬───────────────────┤
│ سیستم            │ زمان     │ دقت      │ RAM              │
├──────────────────┼──────────┼──────────┼───────────────────┤
│ OmniType (فعلی) │ ۳.۲s    │ ۸۵٪     │ ۱۲۰MB            │
│ Windows Voice   │ ۲.۱s    │ ۷۵٪     │ ۸۰MB             │
│ Google Docs     │ ۱.۸s    │ ۹۲٪     │ N/A (وب)         │
│ سیستم پیشنهادی  │ ۰.۸s    │ ۹۵٪     │ ۳۰MB             │
└──────────────────┴──────────┴──────────┴───────────────────┘
```

---

## ۹. نقشه راه و فازبندی

### ۹.۱ فاز ۱: هسته سیستم (هفته ۱-۲)

**اهداف:**
- راه‌اندازی پروژه Rust
- پیاده‌سازی audio capture
- پیاده‌سازی hotkey detection
- System tray

**خروجی‌ها:**
- [ ] Cargo workspace setup
- [ ] WASAPI audio capture (16kHz, mono)
- [ ] Ring buffer با zero-copy
- [ ] Hotkey polling (1ms interval)
- [ ] System tray با آیکون
- [ ] Logging infrastructure

**معیار موفقیت:**
- برنامه اجرا می‌شود و صدا را ضبط می‌کند
- Hotkey به درستی تشخیص داده می‌شود
- RAM < ۵MB

### ۹.۲ فاز ۲: موتورهای ASR (هفته ۳-۴)

**اهداف:**
- Integration با whisper.cpp
- Integration با Web Speech API
- Priority router

**خروجی‌ها:**
- [ ] whisper.cpp bindings
- [ ] Model loading در startup
- [ ] Web Speech API bridge (WebView2)
- [ ] Priority router
- [ ] Health monitor
- [ ] Connection pooling

**معیار موفقیت:**
- تشخیص ۵ ثانیه صوت < ۱ ثانیه
- Failover خودکار بین موتورها
- Cold start < ۲ ثانیه

### ۹.۳ فاز ۳: Post-Processing (هفته ۵)

**اهداف:**
- Persian normalizer
- Dictionary با Aho-Corasick
- RTL text injection

**خروجی‌ها:**
- [ ] SIMD-optimized normalizer
- [ ] Aho-Corasick dictionary
- [ ] SendInput text injection
- [ ] Clipboard backup/restore
- [ ] Focus restoration

**معیار موفقیت:**
- دقت فارسی > ۹۵٪
- Typing latency < ۱۰۰ms
- بدون تداخل با clipboard کاربر

### ۹.۴ فاز ۴: رابط کاربری (هفته ۶)

**اهداف:**
- Settings window
- Floating orb
- Notifications

**خروجی‌ها:**
- [ ] egui/iced integration
- [ ] Settings panel
- [ ] Floating indicator
- [ ] Toast notifications
- [ ] Theme support

**معیار موفقیت:**
- UI responsive (۶۰ FPS)
- تنظیمات persistent
- زیبایی بصری

### ۹.۵ فاز ۵: Polish و Release (هفته ۷-۸)

**اهداف:**
- Installer
- Auto-updater
- Documentation
- Testing

**خروجی‌ها:**
- [ ] WiX installer
- [ ] GitHub Actions CI/CD
- [ ] Auto-updater
- [ ] Documentation کامل
- [ ] Unit tests (> ۸۰٪ coverage)
- [ ] Integration tests
- [ ] Benchmarks

**معیار موفقیت:**
- نصب آسان < ۲ دقیقه
- بدون کرش در تست ۷ روزه
- Documentation کامل

### ۹.۶ نمودار گانت (Gantt Chart)

```
هفته:  ۱    ۲    ۳    ۴    ۵    ۶    ۷    ۸
       ├────┼────┼────┼────┼────┼────┼────┤
فاز ۱  ████████████
فاز ۲            ████████████
فاز ۳                        ████████
فاز ۴                              ████████
فاز ۵                                    ████████████
```

---

## ۱۰. تحلیل ریسک

### ۱۰.۱ ریسک‌های فنی

| ریسک                        | احتمال   | تأثیر | راه‌حل                    |
| --------------------------- | -------- | ----- | ------------------------ |
| **WebView2 در دسترس نبودن** | کم       | زیاد  | Fallback به whisper.cpp  |
| **whisper.cpp کند روی CPU** | متوسط    | متوسط | استفاده از مدل‌های کوچک‌تر |
| **Web Speech API محدودیت**  | کم       | زیاد  | Rate limiting سمت کاربر  |
| **WASAPI driver issues**    | کم       | زیاد  | Fallback به shared mode  |
| **Memory leak**             | کم       | زیاد  | Profiling منظم           |
| **مشکل در Rust bindings**   | بسیار کم | زیاد  | استفاده از bindings رسمی |

### ۱۰.۲ ریسک‌های پروژه

| ریسک                   | احتمال | تأثیر | راه‌حل                     |
| ---------------------- | ------ | ----- | ------------------------- |
| **تأخیر در توسعه**     | متوسط  | متوسط | MVP-first approach        |
| **عدم پذیرش کاربر**    | کم     | زیاد  | User testing زودهنگام     |
| **تغییر API موتورها**  | متوسط  | متوسط | Abstraction layer         |
| **مشکلات لایسنس**      | کم     | زیاد  | بررسی دقیق لایسنس‌ها       |
| **عدم سازگاری ویندوز** | کم     | زیاد  | Testing روی نسخه‌های مختلف |

### ۱۰.۳ ریسک‌های خارجی

| ریسک                        | احتمال | تأثیر | راه‌حل                |
| --------------------------- | ------ | ----- | -------------------- |
| **تغییر قوانین حریم خصوصی** | کم     | زیاد  | شفافیت در داده‌ها     |
| **رقبای جدید**              | متوسط  | متوسط | تمرکز بر کیفیت       |
| **تغییر سیاست‌های رایگان**   | متوسط  | زیاد  | چندلایه بودن موتورها |

---

## ۱۱. منابع و مراجع

### ۱۱.۱ مستندات رسمی

- [Rust Programming Language](https://doc.rust-lang.org/book/)
- [Tokio Async Runtime](https://tokio.rs/)
- [whisper.cpp Documentation](https://github.com/ggerganov/whisper.cpp)
- [WASAPI Documentation](https://docs.microsoft.com/en-us/windows/win32/coreaudio/wasapi)
- [Web Speech API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Speech_API)

### ۱۱.۲ کتابخانه‌های کلیدی

- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio
- [whisper-rs](https://github.com/tazz4843/whisper-rs) - Rust bindings
- [egui](https://github.com/emilk/egui) - Immediate mode GUI
- [reqwest](https://github.com/seanmonstar/reqwest) - HTTP client
- [ort](https://github.com/pykeio/ort) - ONNX Runtime

### ۱۱.۳ مقالات و پژوهش‌ها

- "Silero VAD: Pre-trained Voice Activity Detector" - Silero Team
- "Whisper: Robust Speech Recognition via Large-Scale Weak Supervision" - OpenAI
- "Real-time Speech Recognition on Edge Devices" - IEEE 2024
- "Low-latency Audio Processing with WASAPI" - Microsoft Research

### ۱۱.۴ پروژه‌های مشابه

- [Vosk](https://alphacephei.com/vosk/) - Offline speech recognition
- [Mozilla DeepSpeech](https://github.com/mozilla/DeepSpeech) - Open source ASR
- [Talon Voice](https://talonvoice.com/) - Voice control
- [SuperWhisper](https://superwhisper.com/) - Commercial PTT

---

## ۱۲. قدم‌های بعدی برای تحقیق

### ۱۲.۱ تحقیقات فنی اولویت‌دار

#### ۱. بنچمارک دقیق whisper.cpp روی سخت‌افزار هدف

**سؤال:** چه مدل whisper.cpp بهترین trade-off بین سرعت و دقت را دارد؟

**روش:**
- تست مدل‌های `tiny`, `base`, `small`, `medium`, `large-v3`
- اندازه‌گیری latency، accuracy، RAM/VRAM
- تست روی CPU و GPU

**خروجی مورد انتظار:**
- جدول مقایسه مدل‌ها
- توصیه برای مدل پیش‌فرض

#### ۲. ارزیابی Web Speech API برای فارسی

**سؤال:** آیا Web Speech API واقعاً ۹۸٪ دقت دارد برای فارسی؟

**روش:**
- جمع‌آوری dataset تست (۱۰۰ جمله فارسی)
- تست روی Chrome، Edge، Firefox
- مقایسه با whisper.cpp

**خروجی مورد انتظار:**
- دقت واقعی هر مرورگر
- محدودیت‌های شناسایی‌شده

#### ۳. بهینه‌سازی Ring Buffer

**سؤال:** بهترین سایز ring buffer برای تعادل latency و memory چیست؟

**روش:**
- تست سایزهای مختلف (۱s، ۵s، ۱۰s، ۳۰s)
- اندازه‌گیری latency و memory usage
- Stress test با ضبط طولانی

**خروجی مورد انتظار:**
- سایز بهینه
- الگوریتم adaptive sizing

#### ۴. مقایسه VAD algorithms

**سؤال:** Silero VAD در مقابل WebRTC VAD کدام بهتر است؟

**روش:**
- تست هر دو روی dataset مشابه
- اندازه‌گیری false positive/negative
- مقایسه CPU usage

**خروجی مورد انتظار:**
- توصیه برای VAD پیش‌فرض
- fallback strategy

### ۱۲.۲ تحقیقات UX

#### ۱. User Study روی PTT فعلی

**سؤال:** کاربران چه مشکلاتی با PTT فعلی دارند؟

**روش:**
- مصاحبه با ۱۰ کاربر
- ضبط جلسات استفاده
- تحلیل pain points

**خروجی مورد انتظار:**
- لیست مشکلات اولویت‌دار
- user personas

#### ۲. تست Hotkey Preferences

**سؤال:** کاربران چه کلیدهای میانبری را ترجیح می‌دهند؟

**روش:**
- نظرسنجی از ۱۰۰ کاربر
- تست A/B روی گزینه‌های مختلف
- تحلیل ergonomic

**خروجی مورد انتظار:**
- hotkey پیش‌فرض
- customization options

### ۱۲.۳ تحقیقات بازار

#### ۱. تحلیل رقبا

**سؤال:** رقبای اصلی چه ویژگی‌هایی دارند و چه قیمت‌هایی دارند؟

**روش:**
- بررسی ۱۰ رقیب اصلی
- مقایسه feature matrix
- تحلیل pricing

**خروجی مورد انتظار:**
- competitive advantage ما
- pricing strategy

#### ۲. بررسی بازار هدف

**سؤال:** چه کسانی به این ابزار نیاز دارند و چقدر حاضرند بپردازند؟

**روش:**
- تحلیل demographics
- نظرسنجی willingness to pay
- بررسی use cases

**خروجی مورد انتظار:**
- target market definition
- monetization strategy

### ۱۲.۴ اولویت‌بندی تحقیقات

```
فوری (هفته ۱-۲):
  ├─ بنچمارک whisper.cpp
  ├─ ارزیابی Web Speech API
  └─ تست VAD algorithms

کوتاه‌مدت (هفته ۳-۴):
  ├─ User Study
  ├─ Hotkey Preferences
  └─ Ring Buffer optimization

میان‌مدت (ماه ۲):
  ├─ تحلیل رقبا
  └─ بررسی بازار هدف
```

---

## 📝 پیوست‌ها

### پیوست A: گلاسری اصطلاحات

| اصطلاح        | تعریف                                             |
| ------------- | ------------------------------------------------- |
| **PTT**       | Push-to-Talk - فشردن کلید برای صحبت               |
| **ASR**       | Automatic Speech Recognition - تشخیص خودکار گفتار |
| **VAD**       | Voice Activity Detection - تشخیص فعالیت صوتی      |
| **WER**       | Word Error Rate - نرخ خطای کلمه                   |
| **WASAPI**    | Windows Audio Session API                         |
| **GIL**       | Global Interpreter Lock (در Python)               |
| **Zero-Copy** | انتقال داده بدون کپی کردن                         |
| **Streaming** | پردازش chunk-by-chunk به جای batch                |

### پیوست B: محاسبات تخمینی

**محاسبه RAM مصرفی:**
```
برنامه Rust پایه:          ۳ MB
whisper.cpp (base model): ۵۰۰ MB (آفلاین)
Ring buffer (۳۰s):         ۲ MB
WebView2:                  ۳۰ MB
GUI (egui):                ۲۰ MB
────────────────────────────────────
جمع (با مدل محلی):        ۵۵۵ MB
جمع (آنلاین):             ۵۵ MB
```

**محاسبه latency:**
```
Hotkey detection:          ۱ ms
WASAPI setup:              ۸ ms
VAD processing:            ۵ ms (per chunk)
ASR (online):              ۵۰۰ ms
ASR (offline):             ۸۰۰ ms
Post-processing:           ۱۰ ms
Text injection:            ۵ ms
────────────────────────────────────
جمع (online):              ۵۲۹ ms
جمع (offline):             ۸۲۹ ms
```

### پیوست C: چک‌لیست قبل از شروع توسعه

- [ ] تأیید معماری نهایی
- [ ] انتخاب نهایی کتابخانه‌ها
- [ ] راه‌اندازی environment توسعه
- [ ] تهیه dataset تست
- [ ] تعریف acceptance criteria برای هر فاز
- [ ] برنامه‌ریزی جلسات review هفتگی
- [ ] راه‌اندازی CI/CD
- [ ] تهیه documentation template

---

## ✅ تأییدیه

این پروپوزال به عنوان سند پایه برای فاز تحقیق و توسعه ارائه می‌شود. پس از تکمیل تحقیقات ذکر شده در بخش ۱۲، این سند به‌روزرسانی خواهد شد تا شامل یافته‌های جدید و تنظیمات لازم باشد.

**تاریخ:** ۱۳ سپتامبر ۲۰۲۶  
**نسخه:** ۱.۰  
**وضعیت:** آماده برای review

---

## 🔄 تاریخچه تغییرات

| نسخه | تاریخ      | تغییرات    |
| ---- | ---------- | ---------- |
| ۱.۰  | ۲۰۲۶-۰۹-۱۳ | نسخه اولیه |

---

**پایان سند**