# S1 — تحویل شناسهٔ جلسه و چرخهٔ عمر

بسته: `S1` · موج ۱ · وضعیت: **آمادهٔ ادغام** (تأیید جزئی — بند ۶ را ببینید)

## خلاصهٔ رفتار

هر دیکته حالا یک `SessionId` دارد و هر تکه‌اش یک `ChunkId`. فاصلهٔ «پایان ضبط» و
«پایان جلسه» جدا شده، پس **نتیجهٔ قطعهٔ پایانی دیگر «دیررس» قلم نمی‌خورد**، و نتیجه‌ای که
بعد از لغو یا پایان جلسه برسد **درج نمی‌شود**.

## فایل‌های تغییرکرده

| فایل | چه |
|---|---|
| [state/session.rs](../../voice-ptt/src/state/session.rs) | `SessionId`، `ChunkId`، `SessionKind`، `SessionPhase`، فهرست جلسات باز و بستهٔ اخیر، و قواعد چرخهٔ عمر + ۷ تست |
| [state/machine.rs](../../voice-ptt/src/state/machine.rs) | سیم‌کشی شناسه‌ها، بازرسی پذیرش در مرز درج، `EmitOutcome::NotAttempted`، `state_after_emit` + ۱ تست |

`state/mod.rs` دست‌نخورده ماند: هر دو نوع داخل `session.rs` تعریف شده‌اند و همان مسیر
ماژول را دارند.

## قرارداد ارائه‌شده

[CONTRACTS.md بند ۱](CONTRACTS.md) — با دو اصلاح نسبت به نسخهٔ اولیه، چون پیاده‌سازی
چیزی را آشکار کرد که متن قرارداد جلوتر از واقعیت نوشته بود:

1. **دو حالت بستن، نه سه.** متن می‌گفت «جلسهٔ تازه هم جلسهٔ قبلی را می‌بندد»، ولی قاعدهٔ
   ۷ (نتیجهٔ جلسهٔ قبلی حفظ شود) خلاف آن است. حالا: جلسه فقط با `result_arrived`
   (**Completed**) یا `cancelled` (**Cancelled**) بسته می‌شود؛ شروع جلسهٔ تازه فقط
   «دیگر جاری نیست» را عوض می‌کند.
2. **`SessionKind` دو حالت دارد، نه سه.** «قطعه‌ای» شکلِ جلسه است نه راهِ بازکردن آن؛
   جلسه‌ای که به‌صورت قطعه‌ای می‌آید، باز هم فشار-وتاپ است. جدا کردن این دو جلوی
   سه‌حالته‌ای را می‌گیرد که هیچ‌جا مصرف‌کننده نداشت.

امضای واقعی:

```rust
pub struct SessionId(pub u64);            // از ۱، یکتا در فرایند
pub struct ChunkId(pub u32);             // از ۱ در هر جلسه
pub enum  SessionKind { PushToTalk, HandsFree }
pub enum  SessionPhase { Recording, AwaitingResult, Completed, Cancelled }

impl SessionDriver {
    pub fn open_session(&mut self, capture_open: bool) -> Option<SessionId>;
    pub fn next_chunk(&mut self, id: SessionId) -> Option<ChunkId>;
    pub fn recording_stopped(&mut self, id: SessionId);   // → AwaitingResult
    pub fn result_arrived(&mut self, id: SessionId);      // → Completed
    pub fn cancelled(&mut self);                          // → Cancelled
    pub fn accepts_result(&self, id: SessionId) -> Result<(), &'static str>;
    pub fn current_session(&self) -> Option<SessionId>;
    pub fn phase_of(&self, id: SessionId) -> Option<SessionPhase>;
    pub fn kind_of(&self, id: SessionId) -> Option<SessionKind>;
}
```

`accepts_result` یک تابع است نه یک bool + یک دلیل جدا: دلیل دقیقاً همان‌جا لازم است که
تصمیم گرفته می‌شود، و دو بار خواندن وضعیت می‌توانست بین‌شان اختلاف بیندازد.

## نکتهٔ رفتاری که باید بدانید

پرچم `recording` حالا **فقط** از سخت‌افزار می‌آید. پیش از این، `decide()` با دیدن
`Effect::BeginRecording` خودش پرچم را بالا می‌برد و `began_recording(open)` بعداً آن را
تأیید می‌کرد؛ یعنی «در حال ضبط» قبل از اینکه میکروفون باز شود یک ادعا بود. حالا اگر
باز نشود، **اصلاً جلسه‌ای ساخته نمی‌شود** و پرچم پایین می‌ماند — همان چیزی که کامنتِ
روی `recording` از قبل ادعا می‌کرد.

## محل‌های اتصال برای بسته‌های بعدی

| بسته | چه می‌گیرد | کجا وصل می‌شود |
|---|---|---|
| `T2` | `accepts_result` باید در مرز درج هم سؤال شود | [injector.rs](../../voice-ptt/src/output/injector.rs) — `InjectRequest` بگیرد `session` و `chunk`، و `EmitOutcome::NotAttempted` روی `InjectOutcome::NotAttempted` نگاشت شود |
| `V1` | `SessionId` برای مالکیت پیش‌نویس | `state/draft.rs` (تازه) |
| `R1` | کلید یکتای `(session, chunk)` برای جلوگیری از درج دوباره | `state/recovery.rs` (تازه) |

## تست و نتیجه

| بررسی | نتیجه |
|---|---|
| `cargo test` | **۳۴۷ پاس** (۳۳۹ → ۳۴۷؛ ۸ تست تازه) |
| `cargo clippy --all-targets` | صفر هشدار |
| `rustfmt --config skip_children=true` | تمیز |
| `mutation-check-session.sh` | **۱۷/۱۷ CAUGHT**، بدون `MISSED`/`NOBUILD`/`SKIP` |

هشت تست تازه، و مهم‌ترینشان: `stopping_the_microphone_does_not_close_the_session` (که
دقیقاً همان باگی را می‌گیرد که قرارداد مرحلهٔ قبل داشت)،
`a_cancelled_session_refuses_its_own_late_result`،
`starting_a_new_session_keeps_the_previous_one_usable`،
`a_failed_microphone_gets_no_identity`،
`closed_sessions_are_remembered_only_recently` (کرانِ حافظه)، و
`a_refused_result_is_not_shown_as_an_error`.

دو کاناری تازه (`C12`–`C17`) هر تصمیم تازه را قفل می‌کنند. `C17` بار اول **MISSED** داد —
یعنی تصمیم «نتیجهٔ ردشده نشان خطا نگیرد» هیچ تستی نداشت؛ به همین دلیل به
`state_after_emit` تبدیل شد (و قابل تست شد) و تست خورد.

## تأییدنشده‌ها و محدودیت‌ها

1. **بازرسی مرز درج تست خودکار ندارد.** کد در `machine::emit` درست است، ولی فقط با
   صوت و موتور زنده اجرا می‌شود. کاناری `C18` را **عمداً نساختم** چون مجبور می‌شد
   برای دلیل غلط `MISSED` بدهد؛ این شکاف در خود فایل کاناری ثبت شده است.
2. **لغو حین پردازش هنوز از بیرون اثبات نشده.** کد حالا جلسه را `Cancelled` می‌کند و
   نتیجهٔ دیررس رد می‌شود، ولی اینکه روی یک نشست واقعی هم درست کار کند نیازمند
   دیکتهٔ واقعی و میکروفون است.
3. **صف بین‌جلسه‌ای هنوز پیاده نشده.** قاعدهٔ ۷ می‌گوید ترتیب به زمان پاسخ وابسته
   نباشد؛ آنچه اینجا هست هر جلسه را مستقل نگه می‌دارد و نتیجهٔ جلسهٔ قبلی معمولاً
   زودتر می‌آید. صف واقعی کارِ `T2` است، چون همان‌جاست که دو نتیجه به یک نقطهٔ درج
   می‌رسند.
4. **حافظهٔ «۸ بستهٔ اخیر» یک حد تجربی است.** اگر روزی بیش از هشت جلسه هم‌زمان در
   انتظار باشد (امروز ممکن نیست)، پاسخِ قدیمی «ناشناخته» می‌شود که باز هم رد می‌شود
   ولی دلیلش دقیق نیست.
5. **کلید قطعه در `process_chunk` هنوز از یک منبع می‌آید**: `next_chunk` روی همان جلسه.
   اگر روزی قطعه‌ای دوباره پردازش شود (تلاش مجددِ تبدیل که طبق تصمیم ۴ حذف شد)، شماره
   مصرف‌شده و شمارهٔ تازه فرق خواهند داشت و این قرارداد باید صریح بنویسد کدام درست است.
6. **`run()` هنوز ۴۰۰+ خط است** و بخش GUI و ASR در [lib.rs](../../voice-ptt/src/lib.rs)
   (۵۵۵ خط) است. این بسته آن‌ها را کوچک نکرد.

## پیشنهاد بازگشت

`git revert` همین یک کامیت. نکتهٔ ریسک: `decide()` دیگر پرچم را بالا نمی‌برد، پس اگر
برگردانده شود باید تست‌های جلسه هم برگردند — یعنی این تغییر و آن کاناری یک واحد هستند.