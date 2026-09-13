# 🔬 برنامه تحقیقاتی جامع برای ساخت رودمپ کامل

بر اساس تحلیل‌های قبلی، این **۸ حوزه کلیدی** نیاز به تحقیق فوری و عمیق دارند تا بتوانیم یک رودمپ واقع‌بینانه و اجرایی بسازیم. هر حوزه را با جزئیات کامل بررسی می‌کنم.

---

## 🎯 اولویت‌بندی حوزه‌های تحقیقاتی

```
┌─────────────────────────────────────────────────────────┐
│              ماتریس اولویت تحقیقات                       │
├──────────────────────┬──────────────────┬──────────────┤
│ حوزه تحقیق           │ تأثیر بر پروژه   │ فوریت        │
├──────────────────────┼──────────────────┼──────────────┤
│ ۱. Web Speech API    │ 🔴 حیاتی         │ ⚡ فوری       │
│ ۲. whisper.cpp      │ 🔴 حیاتی         │ ⚡ فوری       │
│ ۳. WASAPI Pipeline  │ 🟡 مهم           │ ⚡ فوری       │
│ ۴. VAD Algorithms   │ 🟡 مهم           │ 📅 هفته ۱     │
│ ۵. Free Tier Limits │ 🔴 حیاتی         │ 📅 هفته ۱     │
│ ۶. Rust Ecosystem   │ 🟡 مهم           │ 📅 هفته ۲     │
│ ۷. UX Research      │ 🟢 تکمیلی        │ 📅 هفته ۳     │
│ ۸. Integration POC  │ 🔴 حیاتی         │ 📅 هفته ۲     │
└──────────────────────┴──────────────────┴──────────────┘
```

---

## ۱. 🔴 تحقیق حیاتی: Web Speech API برای فارسی

### چرا این حوزه حیاتی است؟
Web Speech API **پایه اصلی استراتژی رایگان بودن** ماست. اگر این کار نکند، کل معماری باید بازنگری شود.

### سؤالات کلیدی که باید پاسخ دهیم:

#### الف) سؤالات فنی
```
۱. آیا Web Speech API در WebView2 واقعاً کار می‌کند؟
   - دسترسی از طریق JavaScript bridge
   - محدودیت‌های امنیتی
   - نیاز به HTTPS

۲. دقت واقعی برای زبان فارسی چقدر است؟
   - Word Error Rate (WER)
   - Character Error Rate (CER)
   - Sentence Accuracy

۳. محدودیت‌های نرخ (Rate Limiting) چیست؟
   - حداکثر تعداد درخواست در دقیقه/ساعت/روز
   - آیا IP-based است یا user-based؟
   - رفتار در صورت عبور از محدودیت

۴. تأخیر واقعی چقدر است؟
   - Time to First Word
   - Full Recognition Latency
   - Streaming vs Batch
```

#### ب) سؤالات عملیاتی
```
۱. آیا نیاز به نصب جداگانه WebView2 Runtime است؟
۲. آیا در ویندوز ۱۰ هم کار می‌کند یا فقط ۱۱؟
۳. رفتار در صورت نبود اینترنت چیست؟
۴. آیا می‌توان از آن در حالت headless استفاده کرد؟
```

### روش تحقیق:

#### مرحله ۱: Proof of Concept (۲ روز)
```javascript
// ساخت یک HTML ساده برای تست
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Persian Speech Test</title>
</head>
<body>
    <button id="startBtn">شروع ضبط</button>
    <div id="result"></div>
    
    <script>
        const recognition = new webkitSpeechRecognition();
        recognition.lang = 'fa-IR';
        recognition.continuous = true;
        recognition.interimResults = true;
        
        recognition.onresult = (event) => {
            const transcript = event.results[event.results.length - 1][0].transcript;
            document.getElementById('result').textContent = transcript;
            
            // ارسال به Rust از طریق IPC
            window.chrome.webview.postMessage({
                type: 'transcript',
                text: transcript
            });
        };
        
        document.getElementById('startBtn').onclick = () => {
            recognition.start();
        };
    </script>
</body>
</html>
```

```rust
// تست WebView2 در Rust
use webview2::WebView;

fn main() -> Result<()> {
    let webview = WebView::new("file:///test.html")?;
    
    webview.on_message(|msg| {
        let data: Transcript = serde_json::from_str(&msg)?;
        println!("Recognized: {}", data.text);
        Ok(())
    })?;
    
    webview.run()?;
    Ok(())
}
```

#### مرحله ۲: بنچمارک دقت (۳ روز)
```
Dataset تست:
- ۱۰۰ جمله فارسی روزمره
- ۵۰ جمله با اصطلاحات فنی (Python, API, Docker)
- ۳۰ جمله با کلمات انگلیسی در متن فارسی
- ۲۰ جمله در محیط پر سروصدا

معیارهای اندازه‌گیری:
- Word Error Rate (WER) = (S + D + I) / N
- Character Error Rate (CER)
- Real-time Factor (RTF)
- First Word Latency
```

#### مرحله ۳: تست محدودیت‌ها (۲ روز)
```
سناریوهای تست:
۱. ارسال ۱۰۰۰ درخواست در ۱ دقیقه
۲. ارسال ۱۰۰۰۰ درخواست در ۱ روز
۳. تست با IPهای مختلف (VPN)
۴. تست همزمان از چند tab
۵. تست در ساعات مختلف روز

داده‌های جمع‌آوری شده:
- تعداد موفق/ناموفق
- زمان پاسخ هر درخواست
- کدهای خطا (۴۲۹، ۵۰۳، ...)
- مدت زمان block شدن
```

### خروجی مورد انتظار:

```markdown
# گزارش تحقیق Web Speech API

## خلاصه اجرایی
- ✅ کار می‌کند / ❌ کار نمی‌کند
- دقت: XX٪ برای فارسی
- محدودیت: XXX درخواست در روز
- تأخیر: XXX ms

## نتایج بنچمارک
| متریک | مقدار |
|-------|-------|
| WER | X.X٪ |
| CER | X.X٪ |
| First Word Latency | XXX ms |
| Full Recognition | XXX ms |

## محدودیت‌ها
- Rate limit: XXX req/day
- Concurrent: X sessions
- IP block: XX minutes

## توصیه نهایی
[استفاده به عنوان موتور اصلی / fallback / غیرقابل استفاده]
```

---

## ۲. 🔴 تحقیق حیاتی: whisper.cpp Performance

### چرا این حوزه حیاتی است؟
whisper.cpp **موتور آفلاین اصلی** ماست. اگر کند باشد یا دقت کافی نداشته باشد، باید استراتژی را تغییر دهیم.

### سؤالات کلیدی:

#### الف) بنچمارک مدل‌ها
```
۱. کدام مدل بهترین trade-off بین سرعت و دقت دارد؟
   - tiny (۳۹M params)
   - base (۷۴M params)
   - small (۲۴۴M params)
   - medium (۷۶۹M params)
   - large-v3 (۱۵۵۰M params)
   - large-v3-turbo (۸۰۹M params)

۲. عملکرد روی سخت‌افزارهای مختلف:
   - CPU only (Intel i5, i7, i9)
   - CPU + GPU (NVIDIA RTX 3060, 4070)
   - RAM usage per model
   - VRAM usage per model

۳. تأخیر برای طول‌های مختلف صوت:
   - ۱ ثانیه
   - ۵ ثانیه
   - ۱۰ ثانیه
   - ۳۰ ثانیه
```

#### ب) بهینه‌سازی‌ها
```
۱. آیا quantization (int8, int4) تأثیر زیادی بر دقت دارد؟
۲. آیا batch processing کمک می‌کند؟
۳. بهترین تنظیمات برای فارسی چیست؟
   - language="fa"
   - task="transcribe" vs "translate"
   - beam_size
   - temperature
   - initial_prompt
```

### روش تحقیق:

#### مرحله ۱: راه‌اندازی محیط تست (۱ روز)
```bash
# نصب whisper.cpp
git clone https://github.com/ggerganov/whisper.cpp
cd whisper.cpp
make -j

# دانلود مدل‌ها
./models/download-ggml-model.sh tiny
./models/download-ggml-model.sh base
./models/download-ggml-model.sh small
./models/download-ggml-model.sh medium
./models/download-ggml-model.sh large-v3
./models/download-ggml-model.sh large-v3-turbo
```

#### مرحله ۲: ساخت Benchmark Suite (۳ روز)
```rust
// benchmark.rs
use std::time::Instant;
use whisper_rs::{WhisperContext, FullParams};

struct BenchmarkResult {
    model: String,
    audio_duration: f32,
    inference_time: Duration,
    rtf: f32,  // Real-time Factor
    ram_usage: usize,
    vram_usage: usize,
    wer: f32,
}

fn benchmark_model(model_path: &str, audio_files: &[PathBuf]) -> Vec<BenchmarkResult> {
    let ctx = WhisperContext::new(model_path)?;
    let mut results = vec![];
    
    for audio in audio_files {
        let samples = load_audio(audio)?;
        let duration = samples.len() as f32 / 16000.0;
        
        // Warm-up
        let _ = ctx.transcribe(&samples[..16000]);
        
        // Benchmark
        let start = Instant::now();
        let result = ctx.transcribe(&samples);
        let elapsed = start.elapsed();
        
        let rtf = elapsed.as_secs_f32() / duration;
        let ram = get_process_ram()?;
        let vram = get_gpu_ram()?;
        let wer = calculate_wer(&result.text, &audio.ground_truth)?;
        
        results.push(BenchmarkResult {
            model: model_path.to_string(),
            audio_duration: duration,
            inference_time: elapsed,
            rtf,
            ram_usage: ram,
            vram_usage: vram,
            wer,
        });
    }
    
    results
}
```

#### مرحله ۳: Dataset تست فارسی (۵ روز)
```
ساخت dataset:
- ۲۰۰ جمله فارسی با ground truth
- طول‌های مختلف (۱s تا ۳۰s)
- محیط‌های مختلف (آرام، پر سروصدا)
- شامل اصطلاحات فنی

فرمت:
audio/
├── test_001.wav (۳s, "سلام حالت چطوره")
├── test_002.wav (۵s, "من می‌خوام Python یاد بگیرم")
├── ...
└── ground_truth.json
    {
        "test_001.wav": "سلام حالت چطوره",
        "test_002.wav": "من می‌خوام Python یاد بگیرم",
        ...
    }
```

### خروجی مورد انتظار:

```markdown
# گزارش بنچمارک whisper.cpp

## جدول مقایسه مدل‌ها (CPU - Intel i7-12700K)

| مدل | سایز | RAM | سرعت (۵s audio) | RTF | WER فارسی |
|------|------|-----|------------------|-----|-----------|
| tiny | 75MB | 200MB | 0.8s | 0.16 | 35٪ |
| base | 142MB | 400MB | 1.2s | 0.24 | 25٪ |
| small | 466MB | 1GB | 2.5s | 0.50 | 18٪ |
| medium | 1.5GB | 3GB | 6.0s | 1.20 | 12٪ |
| large-v3 | 3GB | 6GB | 12s | 2.40 | 8٪ |
| large-v3-turbo | 1.6GB | 3GB | 4.0s | 0.80 | 10٪ |

## جدول مقایسه مدل‌ها (GPU - RTX 4070)

| مدل | VRAM | سرعت (۵s audio) | RTF | WER فارسی |
|------|------|------------------|-----|-----------|
| tiny | 500MB | 0.3s | 0.06 | 35٪ |
| base | 800MB | 0.5s | 0.10 | 25٪ |
| small | 2GB | 1.0s | 0.20 | 18٪ |
| medium | 4GB | 2.5s | 0.50 | 12٪ |
| large-v3 | 8GB | 5.0s | 1.00 | 8٪ |
| large-v3-turbo | 4GB | 1.5s | 0.30 | 10٪ |

## توصیه
- **پیش‌فرض CPU:** small (تعادل خوب)
- **پیش‌فرض GPU:** large-v3-turbo (دقت بالا، سرعت خوب)
- **حالت سریع:** base (برای CPUهای ضعیف)
```

---

## ۳. 🟡 تحقیق مهم: WASAPI Audio Pipeline

### سؤالات کلیدی:

#### الف) دسترسی و پیاده‌سازی
```
۱. آیا cpal واقعاً به WASAPI Exclusive Mode دسترسی دارد؟
۲. حداقل latency قابل دستیابی چقدر است؟
۳. آیا می‌توان همزمان چند stream داشت؟
۴. رفتار در صورت نبود دستگاه صوتی چیست؟
```

#### ب) عملکرد
```
۱. تأخیر واقعی از میکروفون تا buffer:
   - MME: ~۵۰ms
   - DirectSound: ~۳۰ms
   - WASAPI Shared: ~۲۰ms
   - WASAPI Exclusive: ~۸ms

۲. CPU overhead برای capture:
   - درصد CPU در idle
   - درصد CPU در active capture
   - تأثیر بر سایر برنامه‌ها

۳. Memory usage:
   - Ring buffer size بهینه
   - Zero-copy implementation
```

### روش تحقیق:

#### مرحله ۱: تست cpal (۲ روز)
```rust
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::time::Instant;

fn test_wasapi_latency() -> Result<()> {
    let host = cpal::default_host();
    let device = host.default_input_device().unwrap();
    
    // تست حالت‌های مختلف
    let configs = vec![
        ("MME", HostId::Wasapi, ShareMode::Shared),
        ("WASAPI Shared", HostId::Wasapi, ShareMode::Shared),
        ("WASAPI Exclusive", HostId::Wasapi, ShareMode::Exclusive),
    ];
    
    for (name, host_id, share_mode) in configs {
        let host = cpal::host_from_id(host_id)?;
        let device = host.default_input_device().unwrap();
        
        let config = StreamConfig {
            channels: 1,
            sample_rate: SampleRate(16000),
            buffer_size: BufferSize::Fixed(256),
        };
        
        let start = Instant::now();
        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], info: &InputCallbackInfo| {
                let latency = start.elapsed();
                println!("{}: First callback after {:?}", name, latency);
            },
            |err| eprintln!("Error: {}", err),
            None,
        )?;
        
        stream.play()?;
        std::thread::sleep(Duration::from_secs(5));
    }
    
    Ok(())
}
```

#### مرحله ۲: Ring Buffer Optimization (۳ روز)
```rust
// تست سایزهای مختلف ring buffer
use ringbuf::RingBuffer;

fn benchmark_ring_buffer() {
    let sizes = vec![
        16000 * 1,   // 1 second
        16000 * 5,   // 5 seconds
        16000 * 10,  // 10 seconds
        16000 * 30,  // 30 seconds
        16000 * 60,  // 60 seconds
    ];
    
    for size in sizes {
        let buffer = RingBuffer::<f32>::new(size);
        
        // تست write/read performance
        let start = Instant::now();
        for i in 0..1_000_000 {
            buffer.write(&[0.5; 1024]);
            let _ = buffer.read(&mut [0.0; 1024]);
        }
        let elapsed = start.elapsed();
        
        println!("Size: {} samples, Time: {:?}, Ops/sec: {}", 
                 size, elapsed, 1_000_000 / elapsed.as_secs());
    }
}
```

### خروجی مورد انتظار:

```markdown
# گزارش WASAPI Pipeline

## تأخیر واقعی
| حالت | تأخیر | CPU Usage | پایداری |
|------|-------|-----------|----------|
| MME | 45-55ms | 2٪ | ✅ خوب |
| WASAPI Shared | 18-25ms | 1.5٪ | ✅ خوب |
| WASAPI Exclusive | 6-10ms | 1٪ | ⚠️ ممکن است conflict داشته باشد |

## Ring Buffer Optimization
| سایز | Memory | Write Latency | Read Latency |
|------|--------|---------------|--------------|
| 1s | 64KB | < 1μs | < 1μs |
| 5s | 320KB | < 1μs | < 1μs |
| 10s | 640KB | < 1μs | < 1μs |
| 30s | 1.9MB | < 1μs | < 1μs |

## توصیه
- **حالت پیش‌فرض:** WASAPI Shared (تعادل خوب)
- **حالت Performance:** WASAPI Exclusive (اگر کاربر اجازه دهد)
- **Ring Buffer Size:** 30 seconds (کافی برای هر سناریو)
```

---

## ۴. 🟡 تحقیق مهم: VAD Algorithms

### سؤالات کلیدی:

```
۱. مقایسه Silero VAD vs WebRTC VAD:
   - دقت در محیط‌های مختلف
   - CPU usage
   - Memory footprint
   - Latency

۲. بهترین threshold برای فارسی چیست؟
   - false positive rate
   - false negative rate
   - optimal threshold curve

۳. رفتار در شرایط مختلف:
   - محیط آرام
   - محیط پر سروصدا (کافه، خیابان)
   - میکروفون دور/نزدیک
   - کیفیت میکروفون پایین/بالا
```

### روش تحقیق:

#### مرحله ۱: ساخت Dataset تست (۳ روز)
```
جمع‌آوری audio samples:
- ۱۰۰ نمونه گفتار فارسی
- ۱۰۰ نمونه نویز (کافه، خیابان، فن)
- ۵۰ نمونه ترکیبی (گفتار + نویز)
- ۵۰ نمونه سکوت

Labeling:
- speech: [0.0, 1.0] probability
- noise: [0.0, 1.0]
- silence: [0.0, 1.0]
```

#### مرحله ۲: بنچمارک الگوریتم‌ها (۴ روز)
```rust
use ort::Graph;

struct VADBenchmark {
    algorithm: String,
    accuracy: f32,
    precision: f32,
    recall: f32,
    f1_score: f32,
    cpu_usage: f32,
    latency_ms: f32,
    memory_mb: f32,
}

fn benchmark_silero_vad() -> VADBenchmark {
    let model = Graph::from_file("silero_vad.onnx")?;
    
    // تست روی dataset
    let mut tp = 0;
    let mut fp = 0;
    let mut fn_ = 0;
    let mut tn = 0;
    
    for sample in test_samples {
        let prediction = model.predict(&sample.audio)?;
        let ground_truth = sample.label;
        
        if prediction > 0.5 && ground_truth == "speech" {
            tp += 1;
        } else if prediction > 0.5 && ground_truth == "noise" {
            fp += 1;
        } else if prediction < 0.5 && ground_truth == "speech" {
            fn_ += 1;
        } else {
            tn += 1;
        }
    }
    
    let accuracy = (tp + tn) as f32 / (tp + fp + fn_ + tn) as f32;
    let precision = tp as f32 / (tp + fp) as f32;
    let recall = tp as f32 / (tp + fn_) as f32;
    let f1 = 2.0 * precision * recall / (precision + recall);
    
    VADBenchmark {
        algorithm: "Silero VAD".to_string(),
        accuracy,
        precision,
        recall,
        f1_score: f1,
        cpu_usage: measure_cpu_usage(),
        latency_ms: measure_latency(),
        memory_mb: measure_memory(),
    }
}
```

### خروجی مورد انتظار:

```markdown
# گزارش VAD Algorithms

## مقایسه الگوریتم‌ها
| الگوریتم | Accuracy | Precision | Recall | F1 | CPU | Latency | Memory |
|-----------|----------|-----------|--------|----|-----|---------|--------|
| Silero VAD | 98٪ | 97٪ | 99٪ | 0.98 | 0.5٪ | 0.8ms | 2MB |
| WebRTC VAD | 92٪ | 88٪ | 95٪ | 0.91 | 0.2٪ | 0.3ms | 0.5MB |
| RMS Threshold | 75٪ | 65٪ | 85٪ | 0.73 | 0.1٪ | 0.1ms | 0.1MB |

## Optimal Threshold برای Silero
| Threshold | False Positive | False Negative | F1 |
|-----------|----------------|----------------|-----|
| 0.3 | 15٪ | 2٪ | 0.89 |
| 0.5 | 5٪ | 5٪ | 0.95 |
| 0.7 | 2٪ | 12٪ | 0.92 |
| 0.9 | 1٪ | 25٪ | 0.83 |

## توصیه
- **پیش‌فرض:** Silero VAD با threshold 0.5
- **حالت سریع:** WebRTC VAD (برای CPUهای ضعیف)
- **Fallback:** RMS Threshold (اگر ONNX Runtime در دسترس نبود)
```

---

## ۵. 🔴 تحقیق حیاتی: Free Tier Limits

### چرا این حوزه حیاتی است؟
اگر سرویس‌های رایگان محدودیت‌های سخت داشته باشند، استراتژی "رایگان بودن کامل" شکست می‌خورد.

### سؤالات کلیدی:

#### الف) Web Speech API
```
۱. آیا واقعاً رایگان است یا محدودیت دارد؟
۲. حداکثر تعداد درخواست در روز؟
۳. آیا IP-based است یا user-based؟
۴. رفتار در صورت عبور از محدودیت؟
۵. آیا Google می‌تواند آن را غیرفعال کند؟
```

#### ب) Groq Free Tier
```
۱. محدودیت‌های واقعی:
   - ۳۰۰ درخواست در روز (adعا شده)
   - آیا دقیقه‌ای هم محدودیت دارد؟
   - آیا concurrent محدودیت دارد؟
۲. مدت زمان اعتبار:
   - آیا برای همیشه رایگان است؟
   - آیا نیاز به credit card دارد؟
۳. کیفیت سرویس:
   - آیا throttling وجود دارد؟
   - تأخیر در ساعات اوج؟
```

#### ج) Google Translate Free Endpoint
```
۱. آیا هنوز کار می‌کند؟
۲. محدودیت‌ها:
   - تعداد کاراکتر در روز
   - IP-based blocking
۳. پایداری:
   - آیا Google آن را deprecate کرده؟
```

### روش تحقیق:

#### مرحله ۱: تست Web Speech API (۳ روز)
```javascript
// تست rate limiting
async function stressTest() {
    const results = [];
    
    for (let i = 0; i < 1000; i++) {
        const start = Date.now();
        try {
            const recognition = new webkitSpeechRecognition();
            recognition.lang = 'fa-IR';
            
            await new Promise((resolve, reject) => {
                recognition.onresult = resolve;
                recognition.onerror = reject;
                recognition.start();
                
                setTimeout(() => {
                    recognition.stop();
                    resolve();
                }, 2000);
            });
            
            results.push({
                request: i,
                status: 'success',
                latency: Date.now() - start
            });
        } catch (error) {
            results.push({
                request: i,
                status: 'error',
                error: error.message,
                latency: Date.now() - start
            });
        }
        
        // کمی صبر بین درخواست‌ها
        await new Promise(r => setTimeout(r, 100));
    }
    
    // تحلیل نتایج
    const successCount = results.filter(r => r.status === 'success').length;
    const errorCount = results.filter(r => r.status === 'error').length;
    const avgLatency = results
        .filter(r => r.status === 'success')
        .reduce((sum, r) => sum + r.latency, 0) / successCount;
    
    console.log(`Success: ${successCount}, Error: ${errorCount}`);
    console.log(`Average latency: ${avgLatency}ms`);
    
    return results;
}
```

#### مرحله ۲: تست Groq API (۲ روز)
```python
import requests
import time
from datetime import datetime

def test_groq_limits():
    api_key = "YOUR_GROQ_API_KEY"
    url = "https://api.groq.com/openai/v1/audio/transcriptions"
    
    results = []
    
    # تست ۳۰۰ درخواست در یک روز
    for i in range(350):  # کمی بیشتر از limit
        try:
            with open("test_audio.wav", "rb") as f:
                response = requests.post(
                    url,
                    headers={"Authorization": f"Bearer {api_key}"},
                    files={"file": f},
                    data={
                        "model": "whisper-large-v3-turbo",
                        "language": "fa"
                    }
                )
            
            results.append({
                "request": i,
                "status": response.status_code,
                "time": datetime.now(),
                "latency": response.elapsed.total_seconds()
            })
            
            if response.status_code == 429:
                print(f"Rate limit hit at request {i}")
                break
                
        except Exception as e:
            results.append({
                "request": i,
                "status": "error",
                "error": str(e),
                "time": datetime.now()
            })
        
        time.sleep(1)  # ۱ ثانیه بین درخواست‌ها
    
    # تحلیل
    success = sum(1 for r in results if r["status"] == 200)
    rate_limited = sum(1 for r in results if r["status"] == 429)
    errors = sum(1 for r in results if r["status"] == "error")
    
    print(f"Success: {success}")
    print(f"Rate limited: {rate_limited}")
    print(f"Errors: {errors}")
    
    return results
```

### خروجی مورد انتظار:

```markdown
# گزارش Free Tier Limits

## Web Speech API
| متریک | مقدار |
|-------|-------|
| درخواست‌های موفق | ۱۰۰۰/۱۰۰۰ |
| Rate limit مشاهده شده | ❌ هیچ |
| تأخیر متوسط | ۸۰۰ms |
| پایداری | ✅ عالی |

**نتیجه:** به نظر می‌رسد محدودیت سختی ندارد، اما ممکن است IP-based throttling داشته باشد.

## Groq Free Tier
| متریک | مقدار |
|-------|-------|
| درخواست‌های موفق | ۳۰۰/۳۰۰ |
| درخواست ۳۰۱ | ۴۲۹ Rate Limited |
| Reset time | ۲۴ ساعت |
| تأخیر متوسط | ۳۵۰ms |

**نتیجه:** محدودیت دقیقاً ۳۰۰ درخواست در روز است.

## Google Translate Free
| متریک | مقدار |
|-------|-------|
| وضعیت | ⚠️ ناپایدار |
| Rate limit | ~۱۰۰۰ کاراکتر در دقیقه |
| IP block | بعد از ~۵۰۰۰ کاراکتر |

**نتیجه:** قابل اعتماد نیست، فقط برای fallback استفاده شود.

## توصیه نهایی
۱. **Web Speech API:** موتور اصلی (بدون محدودیت مشاهده شده)
۲. **Groq:** موتور دوم (۳۰۰ درخواست در روز کافی است)
۳. **whisper.cpp:** موتور آفلاین (بدون محدودیت)
۴. **Google Translate:** فقط fallback اضطراری
```

---

## ۶. 🟡 تحقیق مهم: Rust Ecosystem Maturity

### سؤالات کلیدی:

```
۱. آیا کتابخانه‌های مورد نیاز ما به اندازه کافی mature هستند؟
   - cpal: آخرین نسخه، issues باز، activity
   - whisper-rs: آیا به‌روز است؟
   - egui/iced: کدام برای desktop app بهتر است؟
   - web-view: آیا WebView2 را به خوبی پشتیبانی می‌کند؟

۲. آیا binding های Rust برای C/C++ libraries پایدار هستند؟
   - whisper.cpp bindings
   - ONNX Runtime bindings
   - Win32 API bindings

۳. جامعه Rust برای این نوع پروژه چقدر فعال است؟
   - تعداد contributor ها
   - سرعت پاسخ به issues
   - کیفیت documentation
```

### روش تحقیق:

#### مرحله ۱: ارزیابی کتابخانه‌ها (۳ روز)
```bash
# بررسی هر کتابخانه
for crate in cpal whisper-rs egui iced web-view reqwest ort; do
    echo "=== $crate ==="
    cargo info $crate
    cargo search $crate
done

# بررسی GitHub activity
gh repo view RustAudio/cpal --json issues,pullRequests,stargazerCount
gh repo view tazz4843/whisper-rs --json issues,pullRequests,stargazerCount
gh repo view emilk/egui --json issues,pullRequests,stargazerCount
```

#### مرحله ۲: ساخت Mini POC (۵ روز)
```rust
// POC برای تست هر کتابخانه
fn main() -> Result<()> {
    // ۱. تست cpal
    let audio_capture = AudioCapture::new()?;
    
    // ۲. تست whisper-rs
    let whisper = WhisperEngine::new("models/base.bin")?;
    
    // ۳. تست egui
    let gui = GUI::new()?;
    
    // ۴. تست web-view
    let webview = WebViewBridge::new()?;
    
    // ۵. تست reqwest
    let http_client = HttpClient::new()?;
    
    // ۶. تست ort (ONNX)
    let vad = SileroVAD::new()?;
    
    println!("All libraries work! ✅");
    Ok(())
}
```

### خروجی مورد انتظار:

```markdown
# گزارش Rust Ecosystem

## ارزیابی کتابخانه‌ها
| کتابخانه | نسخه | Stars | Issues باز | آخرین commit | Mature؟ |
|-----------|------|-------|------------|----------------|----------|
| cpal | 0.15.3 | 1.2k | 45 | 2 هفته پیش | ✅ بله |
| whisper-rs | 0.3.2 | 450 | 12 | 1 ماه پیش | ⚠️ متوسط |
| egui | 0.28.1 | 7.5k | 120 | 3 روز پیش | ✅ بله |
| iced | 0.13.0 | 5.8k | 280 | 1 هفته پیش | ⚠️ متوسط |
| web-view | 0.8.0 | 800 | 35 | 2 ماه پیش | ⚠️ متوسط |
| reqwest | 0.12.5 | 4.2k | 65 | 1 هفته پیش | ✅ بله |
| ort | 2.0.0 | 1.1k | 28 | 5 روز پیش | ✅ بله |

## توصیه
- **GUI:** egui (mature تر از iced)
- **Audio:** cpal (عالی)
- **ASR:** whisper-rs (کمی ریسک دارد، باید تست شود)
- **WebView:** web-view (ریسک متوسط، POC لازم است)
```

---

## ۷. 🟢 تحقیق تکمیلی: UX Research

### سؤالات کلیدی:

```
۱. کاربران چه مشکلاتی با PTT فعلی دارند؟
۲. چه کلیدهای میانبری را ترجیح می‌دهند؟
۳. چه workflow ای دارند؟
۴. چه ویژگی‌هایی برایشان مهم است؟
```

### روش تحقیق:

#### مرحله ۱: نظرسنجی (۷ روز)
```
ساخت فرم نظرسنجی:
- ۲۰ سؤال
- توزیع در جوامع برنامه‌نویسان فارسی‌زبان
- هدف: ۱۰۰ پاسخ

سؤالات کلیدی:
۱. از چه ابزار تایپ صوتی استفاده می‌کنید؟
۲. بزرگترین مشکل شما چیست؟
۳. چه کلید میانبری را ترجیح می‌دهید؟
۴. چقدر حاضرید برای ابزار بهتر بپردازید؟
۵. چه ویژگی‌هایی برایتان حیاتی است؟
```

#### مرحله ۲: User Interviews (۵ روز)
```
مصاحبه با ۱۰ کاربر:
- برنامه‌نویسان
- نویسندگان
- دانشجویان
- کاربران عادی

هر مصاحبه ۳۰ دقیقه:
- نمایش PTT فعلی
- مشاهده workflow
- جمع‌آوری feedback
```

### خروجی مورد انتظار:

```markdown
# گزارش UX Research

## مشکلات اصلی کاربران
۱. کندی (۷۵٪)
۲. دقت پایین فارسی (۶۰٪)
۳. نیاز به صبر (۵۰٪)
۴. مصرف بالای RAM (۴۰٪)

## ترجیحات Hotkey
۱. Caps Lock (۴۵٪)
۲. Ctrl + Space (۳۰٪)
۳. Scroll Lock (۱۵٪)
۴. کلید سفارشی (۱۰٪)

## ویژگی‌های حیاتی
۱. سرعت (۹۰٪)
۲. دقت (۸۵٪)
۳. رایگان بودن (۸۰٪)
۴. آفلاین (۶۰٪)
۵. UI زیبا (۳۰٪)
```

---

## ۸. 🔴 تحقیق حیاتی: Integration POC

### چرا این حوزه حیاتی است؟
حتی اگر هر کامپوننت به تنهایی کار کند، integration آن‌ها با هم چالش‌های خاص خود را دارد.

### سؤالات کلیدی:

```
۱. آیا می‌توان WebView2 را با Rust bridge کرد؟
۲. چگونه focus restoration را مدیریت کنیم؟
۳. آیا می‌توان همزمان چند stream داشت؟
۴. چگونه state machine را پیاده‌سازی کنیم؟
```

### روش تحقیق:

#### مرحله ۱: ساخت Mini POC (۷ روز)
```rust
// POC کامل برای تست integration
use tokio::sync::mpsc;

struct PTTApp {
    audio_capture: AudioCapture,
    vad: SileroVAD,
    asr_router: PriorityRouter,
    text_injector: TextInjector,
    webview: Option<WebViewBridge>,
}

impl PTTApp {
    async fn run(&mut self) -> Result<()> {
        // State machine
        let mut state = AppState::Idle;
        
        loop {
            match state {
                AppState::Idle => {
                    if self.hotkey_pressed() {
                        state = AppState::Recording;
                        self.audio_capture.start()?;
                    }
                }
                AppState::Recording => {
                    let chunk = self.audio_capture.read_chunk()?;
                    
                    if self.vad.is_speech(&chunk) {
                        // ادامه ضبط
                        self.audio_capture.write_to_buffer(&chunk);
                    } else if self.silence_timeout() {
                        state = AppState::Processing;
                        let audio = self.audio_capture.finalize();
                        
                        // ASR
                        let text = self.asr_router.recognize(&audio).await?;
                        
                        state = AppState::Typing;
                        self.text_injector.inject(&text)?;
                        
                        state = AppState::Idle;
                    }
                }
            }
            
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}
```

### خروجی مورد انتظار:

```markdown
# گزارش Integration POC

## چالش‌های شناسایی‌شده
۱. **WebView2 Bridge:** کار می‌کند ولی پیچیده است
۲. **Focus Restoration:** نیاز به دقت بالا دارد
۳. **Concurrent Streams:** محدودیت در cpal
۴. **State Machine:** پیاده‌سازی ساده است

## راه‌حل‌ها
۱. استفاده از `web-view` crate با custom IPC
۲. ذخیره HWND قبل از ضبط و restore بعد از آن
۳. استفاده از single stream با multiplexing
۴. استفاده از `enum` برای state machine

## زمان تخمینی برای پیاده‌سازی کامل
- فاز ۱: ۲ هفته
- فاز ۲: ۲ هفته
- فاز ۳: ۱ هفته
- فاز ۴: ۱ هفته
- فاز ۵: ۲ هفته
**جمع: ۸ هفته**
```

---

## 📊 خلاصه برنامه تحقیقاتی

```
┌─────────────────────────────────────────────────────────────┐
│              زمان‌بندی کلی تحقیقات (۳ هفته)                │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│ هفته ۱:                                                      │
│ ├─ روز ۱-۲: Web Speech API POC                            │
│ ├─ روز ۳-۵: whisper.cpp Benchmark                          │
│ ├─ روز ۶-۷: WASAPI Pipeline Test                           │
│                                                               │
│ هفته ۲:                                                      │
│ ├─ روز ۸-۱۰: VAD Algorithms Comparison                     │
│ ├─ روز ۱۱-۱۲: Free Tier Limits Testing                     │
│ ├─ روز ۱۳-۱۴: Rust Ecosystem Evaluation                    │
│                                                               │
│ هفته ۳:                                                      │
│ ├─ روز ۱۵-۱۷: Integration POC                              │
│ ├─ روز ۱۸-۱۹: UX Research (Survey)                         │
│ ├─ روز ۲۰-۲۱: Final Analysis & Roadmap                     │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### خروجی نهایی هر هفته:

**هفته ۱:**
- گزارش Web Speech API
- گزارش whisper.cpp Benchmark
- گزارش WASAPI Pipeline

**هفته ۲:**
- گزارش VAD Comparison
- گزارش Free Tier Limits
- گزارش Rust Ecosystem

**هفته ۳:**
- گزارش Integration POC
- گزارش UX Research
- **رودمپ نهایی**

---

## 🎯 قدم بعدی

پس از تکمیل این تحقیقات، خواهیم داشت:

1. ✅ داده‌های واقعی برای هر کامپوننت
2. ✅ شناسایی ریسک‌ها و چالش‌ها
3. ✅ تخمین دقیق زمان توسعه
4. ✅ انتخاب نهایی تکنولوژی‌ها
5. ✅ **رودمپ کامل و اجرایی**

آیا می‌خواهید روی یکی از این حوزه‌ها عمیق‌تر شویم یا برنامه تحقیقاتی را شروع کنیم؟

