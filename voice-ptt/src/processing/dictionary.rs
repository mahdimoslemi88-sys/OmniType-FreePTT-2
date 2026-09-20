//! Technical-term dictionary using Aho-Corasick multi-pattern matching.
//!
//! ASR engines mangle technical terms in Persian speech ("پایتون" →
//! "پاتون", "جاوااسکریپت" → "جاوا اسکریپ"). The dictionary maps the common
//! mis-transcriptions back to the canonical form. Matching is done with
//! Aho-Corasick so hundreds of patterns cost a single pass over the text.

use aho_corasick::AhoCorasick;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One correction rule: replace `from` (in normalized text) with `to`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Correction {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Rich default seed corrections covering software, AI/ML, DevOps,
/// mechanical/petroleum engineering (AITCO domain), and common tech acronyms.
pub fn default_corrections() -> Vec<Correction> {
    const SEED: &[(&str, &str, &str)] = &[
        // ── نرم‌افزار و برنامه‌نویسی (Software & Web) ──────────────────────────
        ("پاتون", "پایتون", "برنامه‌نویسی"),
        ("پايتون", "پایتون", "برنامه‌نویسی"),
        ("پایتن", "پایتون", "برنامه‌نویسی"),
        ("جاوا اسکریپت", "جاوااسکریپت", "برنامه‌نویسی"),
        ("جاوا سکریپت", "جاوااسکریپت", "برنامه‌نویسی"),
        ("تایپ اسکریپت", "تایپ‌اسکریپت", "برنامه‌نویسی"),
        ("تایپسکریپت", "تایپ‌اسکریپت", "برنامه‌نویسی"),
        ("ری اکت", "React", "برنامه‌نویسی"),
        ("ریکت", "React", "برنامه‌نویسی"),
        ("نکست جی اس", "Next.js", "برنامه‌نویسی"),
        ("نکست جی‌اس", "Next.js", "برنامه‌نویسی"),
        ("نود جی اس", "Node.js", "برنامه‌نویسی"),
        ("نود جی‌اس", "Node.js", "برنامه‌نویسی"),
        ("ویو جی اس", "Vue.js", "برنامه‌نویسی"),
        ("انگولار", "Angular", "برنامه‌نویسی"),
        ("انگیولار", "Angular", "برنامه‌نویسی"),
        ("لاراول", "Laravel", "برنامه‌نویسی"),
        ("جنگو", "Django", "برنامه‌نویسی"),
        ("فست ای پی آی", "FastAPI", "برنامه‌نویسی"),
        ("فست‌ای‌پی‌آی", "FastAPI", "برنامه‌نویسی"),
        ("اکسپرس جی اس", "Express.js", "برنامه‌نویسی"),
        ("نست جی اس", "NestJS", "برنامه‌نویسی"),
        ("اسپرینگ بوت", "Spring Boot", "برنامه‌نویسی"),
        ("تیلویند", "Tailwind CSS", "برنامه‌نویسی"),
        ("تیل‌ویند", "Tailwind CSS", "برنامه‌نویسی"),
        ("بوت استرپ", "Bootstrap", "برنامه‌نویسی"),
        ("فلاتر", "Flutter", "برنامه‌نویسی"),
        ("کاتلین", "Kotlin", "برنامه‌نویسی"),
        ("سوئیفت", "Swift", "برنامه‌نویسی"),
        ("گولنگ", "Golang", "برنامه‌نویسی"),
        ("سی پلاس پلاس", "C++", "برنامه‌نویسی"),
        ("سی شارپ", "C#", "برنامه‌نویسی"),
        ("راست", "Rust", "برنامه‌نویسی"),
        ("وب‌پک", "Webpack", "برنامه‌نویسی"),
        ("ویت", "Vite", "برنامه‌نویسی"),
        ("فرانت اند", "فرانت‌اند", "برنامه‌نویسی"),
        ("فرانتند", "فرانت‌اند", "برنامه‌نویسی"),
        ("بک اند", "بک‌اند", "برنامه‌نویسی"),
        ("بکند", "بک‌اند", "برنامه‌نویسی"),
        ("فول استک", "فول‌استک", "برنامه‌نویسی"),
        ("فولستک", "فول‌استک", "برنامه‌نویسی"),

        // ── هوش مصنوعی و علم داده (AI / ML / Data Science) ───────────────────
        ("پای تورچ", "PyTorch", "هوش مصنوعی"),
        ("پایتورچ", "PyTorch", "هوش مصنوعی"),
        ("تنسورفلو", "TensorFlow", "هوش مصنوعی"),
        ("تنسور فلو", "TensorFlow", "هوش مصنوعی"),
        ("هاگینگ فیس", "Hugging Face", "هوش مصنوعی"),
        ("هاگینگ‌فیس", "Hugging Face", "هوش مصنوعی"),
        ("ترنسفورمرز", "Transformers", "هوش مصنوعی"),
        ("ترنسفورمر", "Transformer", "هوش مصنوعی"),
        ("ال ال ام", "LLM", "هوش مصنوعی"),
        ("جی پی تی", "GPT", "هوش مصنوعی"),
        ("چت جی پی تی", "ChatGPT", "هوش مصنوعی"),
        ("اوپن ای آی", "OpenAI", "هوش مصنوعی"),
        ("اوپن‌ای‌آی", "OpenAI", "هوش مصنوعی"),
        ("کلود", "Claude", "هوش مصنوعی"),
        ("جمینای", "Gemini", "هوش مصنوعی"),
        ("جمینی", "Gemini", "هوش مصنوعی"),
        ("پرامپت انجینیرینگ", "Prompt Engineering", "هوش مصنوعی"),
        ("فاین تیون", "Fine-tuning", "هوش مصنوعی"),
        ("فاین‌تیون", "Fine-tuning", "هوش مصنوعی"),
        ("وکتور دیتابیس", "Vector Database", "هوش مصنوعی"),
        ("لنگ چین", "LangChain", "هوش مصنوعی"),
        ("لنگ‌چین", "LangChain", "هوش مصنوعی"),
        ("او ان ان ایکس", "ONNX", "هوش مصنوعی"),
        ("اوپن سی وی", "OpenCV", "هوش مصنوعی"),
        ("مدیا پایپ", "MediaPipe", "هوش مصنوعی"),
        ("پانداس", "Pandas", "هوش مصنوعی"),
        ("نام پای", "NumPy", "هوش مصنوعی"),
        ("سایکیت لرن", "Scikit-learn", "هوش مصنوعی"),
        ("ماشین لرنینگ", "Machine Learning", "هوش مصنوعی"),
        ("دیپ لرنینگ", "Deep Learning", "هوش مصنوعی"),

        // ── دوآپس، پایگاه داده و زیرساخت (DevOps & Databases) ────────────────
        ("داکر", "Docker", "دوآپس"),
        ("داکر کامپوز", "Docker Compose", "دوآپس"),
        ("کوبرنتیز", "Kubernetes", "دوآپس"),
        ("کوبرنتیس", "Kubernetes", "دوآپس"),
        ("کوبراینتس", "Kubernetes", "دوآپس"),
        ("گیتهاب", "گیت‌هاب", "ابزارها"),
        ("گیت هاب", "گیت‌هاب", "ابزارها"),
        ("گیت لب", "GitLab", "ابزارها"),
        ("گیت‌لب", "GitLab", "ابزارها"),
        ("وی اس کد", "VS Code", "ابزارها"),
        ("ویژوال استودیو", "Visual Studio", "ابزارها"),
        ("لینوکس", "لینوکس", "سیستم‌عامل"),
        ("اوبونتو", "Ubuntu", "سیستم‌عامل"),
        ("ترمینال", "ترمینال", "ابزارها"),
        ("دیتابیس", "دیتابیس", "پایگاه داده"),
        ("داده بیس", "دیتابیس", "پایگاه داده"),
        ("پستگرس", "PostgreSQL", "پایگاه داده"),
        ("پستگرس کیو ال", "PostgreSQL", "پایگاه داده"),
        ("مای اس کیو ال", "MySQL", "پایگاه داده"),
        ("مای‌اس‌کیوال", "MySQL", "پایگاه داده"),
        ("مونگو دی بی", "MongoDB", "پایگاه داده"),
        ("مونگو‌دی‌بی", "MongoDB", "پایگاه داده"),
        ("ردیس", "Redis", "پایگاه داده"),
        ("اس کیو لایت", "SQLite", "پایگاه داده"),
        ("الاستیک سرچ", "Elasticsearch", "پایگاه داده"),
        ("کافکا", "Kafka", "دوآپس"),
        ("ربیت ام کیو", "RabbitMQ", "دوآپس"),
        ("انجین ایکس", "Nginx", "دوآپس"),
        ("آپاچی", "Apache", "دوآپس"),
        ("سی آی سی دی", "CI/CD", "دوآپس"),
        ("سی‌آی‌سی‌دی", "CI/CD", "دوآپس"),
        ("پرومتیوس", "Prometheus", "دوآپس"),
        ("گرافانا", "Grafana", "دوآپس"),

        // ── مهندسی، مکانیک، نفت، گاز و پتروشیمی (AITCO Domain) ───────────────
        ("گاسکت", "گسکت", "مهندسی و نفت و گاز"),
        ("اسپیرال وند", "اسپیرال وند (Spiral Wound)", "مهندسی و نفت و گاز"),
        ("اسپیرال‌وند", "اسپیرال وند (Spiral Wound)", "مهندسی و نفت و گاز"),
        ("آر تی جی", "RTJ", "مهندسی و نفت و گاز"),
        ("کلینگریت", "Klingerrit", "مهندسی و نفت و گاز"),
        ("کم پروفایل", "Kammprofile", "مهندسی و نفت و گاز"),
        ("فلانج", "فلنج", "مهندسی و نفت و گاز"),
        ("ولدینگ نک", "Welding Neck", "مهندسی و نفت و گاز"),
        ("بلایند فلنج", "Blind Flange", "مهندسی و نفت و گاز"),
        ("اسلیپ ان", "Slip-on", "مهندسی و نفت و گاز"),
        ("بال ولو", "Ball Valve", "مهندسی و نفت و گاز"),
        ("گیت ولو", "Gate Valve", "مهندسی و نفت و گاز"),
        ("گلوب ولو", "Globe Valve", "مهندسی و نفت و گاز"),
        ("چک ولو", "Check Valve", "مهندسی و نفت و گاز"),
        ("باترفلای ولو", "Butterfly Valve", "مهندسی و نفت و گاز"),
        ("پلاگ ولو", "Plug Valve", "مهندسی و نفت و گاز"),
        ("نیدل ولو", "Needle Valve", "مهندسی و نفت و گاز"),
        ("سیفتی ولو", "Safety Valve", "مهندسی و نفت و گاز"),
        ("کنترل ولو", "Control Valve", "مهندسی و نفت و گاز"),
        ("اکچویتور", "Actuator", "مهندسی و نفت و گاز"),
        ("اکچوئیتور", "Actuator", "مهندسی و نفت و گاز"),
        ("پی اند آی دی", "P&ID", "مهندسی و نفت و گاز"),
        ("پی اند ایدی", "P&ID", "مهندسی و نفت و گاز"),
        ("پی اف دی", "PFD", "مهندسی و نفت و گاز"),
        ("ام تی او", "MTO", "مهندسی و نفت و گاز"),
        ("آر اف کیو", "RFQ", "مهندسی و نفت و گاز"),
        ("ار اف کیو", "RFQ", "مهندسی و نفت و گاز"),
        ("ایزومتریک", "Isometric", "مهندسی و نفت و گاز"),
        ("سی ان سی", "CNC", "اتوماسیون صنعتی"),
        ("پی ال سی", "PLC", "اتوماسیون صنعتی"),
        ("اسکادا", "SCADA", "اتوماسیون صنعتی"),
        ("اچ ام آی", "HMI", "اتوماسیون صنعتی"),
        ("دی سی اس", "DCS", "اتوماسیون صنعتی"),
        ("کد کم", "CAD/CAM", "مهندسی"),
        ("اتوکد", "AutoCAD", "مهندسی"),
        ("سالیدورک", "SolidWorks", "مهندسی"),
        ("سالیدورکز", "SolidWorks", "مهندسی"),
        ("استنلس استیل", "Stainless Steel", "متالورژی"),
        ("کربن استیل", "Carbon Steel", "متالورژی"),
        ("اینکونل", "Inconel", "متالورژی"),
        ("مونل", "Monel", "متالورژی"),
        ("هستلوی", "Hastelloy", "متالورژی"),
        ("دوپلکس", "Duplex", "متالورژی"),
        ("سوپر دوپلکس", "Super Duplex", "متالورژی"),
        ("هیت اکسچنجر", "Heat Exchanger", "تجهیزات صنعتی"),
        ("مبدل حرارتی", "مبدل حرارتی", "تجهیزات صنعتی"),
        ("مخزن تحت فشار", "Pressure Vessel", "تجهیزات صنعتی"),
        ("پایپینگ", "Piping", "مهندسی و نفت و گاز"),
        ("فیتینگ", "Fitting", "مهندسی و نفت و گاز"),
        ("البو", "Elbow", "مهندسی و نفت و گاز"),
        ("ردیوسر", "Reducer", "مهندسی و نفت و گاز"),
        ("آسمه", "ASME", "استانداردها"),
        ("استم", "ASTM", "استانداردها"),
        ("ایزو", "ISO", "استانداردها"),
        ("دین", "DIN", "استانداردها"),
        ("انسی", "ANSI", "استانداردها"),
        ("نیس", "NACE", "استانداردها"),

        // ── مخفف‌ها و مفاهیم عمومی فنی (General Tech Acronyms) ───────────────
        ("اپی آی", "API", "مخفف‌ها"),
        ("ای پی آی", "API", "مخفف‌ها"),
        ("اچ تی ام ال", "HTML", "مخفف‌ها"),
        ("سی اس اس", "CSS", "مخفف‌ها"),
        ("جی اس او ان", "JSON", "مخفف‌ها"),
        ("جیسون", "JSON", "مخفف‌ها"),
        ("یمل", "YAML", "مخفف‌ها"),
        ("اچ تی تی پی", "HTTP", "مخفف‌ها"),
        ("اچ تی پی", "HTTP", "مخفف‌ها"),
        ("اچ تی تی پی اس", "HTTPS", "مخفف‌ها"),
        ("یو آر ال", "URL", "مخفف‌ها"),
        ("اس کیو ال", "SQL", "مخفف‌ها"),
        ("اسکیوال", "SQL", "مخفف‌ها"),
        ("اس اس اچ", "SSH", "مخفف‌ها"),
        ("اس اس ال", "SSL", "مخفف‌ها"),
        ("دی ان اس", "DNS", "مخفف‌ها"),
        ("پول ریکوئست", "Pull Request", "گیت"),
        ("پول ریکوست", "Pull Request", "گیت"),
        ("ای پی آی رست", "REST API", "مخفف‌ها"),
        ("رست ای پی آی", "REST API", "مخفف‌ها"),
        ("گراف کیو ال", "GraphQL", "مخفف‌ها"),
        ("وب سوکت", "WebSocket", "مخفف‌ها"),
        ("وب هوک", "Webhook", "مخفف‌ها"),
    ];

    SEED.iter()
        .filter(|(a, b, _)| a != b)
        .map(|(from, to, category)| Correction {
            from: from.to_string(),
            to: to.to_string(),
            category: Some(category.to_string()),
        })
        .collect()
}

/// Multi-pattern dictionary with longest-match-wins replacement.
pub struct Dictionary {
    rules: Vec<Correction>,
    matcher: AhoCorasick,
    replacements: Vec<String>,
    file_path: Option<PathBuf>,
}

impl Dictionary {
    /// Builds a dictionary from correction rules.
    pub fn new(corrections: Vec<Correction>) -> Self {
        Self::with_path(corrections, None)
    }

    /// Internal builder that pairs rules with an optional source path.
    pub fn with_path(corrections: Vec<Correction>, file_path: Option<PathBuf>) -> Self {
        let (matcher, replacements, clean_rules) = Self::compile_rules(&corrections);
        Self {
            rules: clean_rules,
            matcher,
            replacements,
            file_path,
        }
    }

    /// The default seed dictionary.
    pub fn with_defaults() -> Self {
        Self::new(default_corrections())
    }

    /// Compiles rules into an AhoCorasick matcher and ordered replacements.
    fn compile_rules(
        corrections: &[Correction],
    ) -> (AhoCorasick, Vec<String>, Vec<Correction>) {
        let mut map: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
        for c in corrections {
            let from = c.from.trim();
            let to = c.to.trim();
            if !from.is_empty() && from != to {
                map.insert(from.to_string(), (to.to_string(), c.category.clone()));
            }
        }

        let mut patterns = Vec::with_capacity(map.len());
        let mut replacements = Vec::with_capacity(map.len());
        let mut clean_rules = Vec::with_capacity(map.len());

        for (from, (to, category)) in map {
            patterns.push(from.clone());
            replacements.push(to.clone());
            clean_rules.push(Correction {
                from,
                to,
                category,
            });
        }

        let matcher = if patterns.is_empty() {
            AhoCorasick::builder()
                .build(["\0"])
                .expect("fallback matcher should never fail")
        } else {
            AhoCorasick::builder()
                .match_kind(aho_corasick::MatchKind::LeftmostLongest)
                .build(&patterns)
                .expect("aho-corasick build cannot fail with non-empty patterns")
        };

        (matcher, replacements, clean_rules)
    }

    /// Loads user corrections from a TOML file and merges them with defaults.
    /// File format:
    /// ```toml
    /// [[corrections]]
    /// from = "پاتون"
    /// to   = "پایتون"
    /// ```
    pub fn load_or_default(path: &Path) -> Self {
        let mut corrections = default_corrections();
        if let Ok(content) = std::fs::read_to_string(path) {
            match toml::from_str::<DictionaryFile>(&content) {
                Ok(file) => {
                    // Prepend user rules or merge so user rules take precedence
                    corrections.extend(file.corrections);
                }
                Err(e) => tracing::warn!(%e, path = %path.display(), "invalid dictionary file"),
            }
        }
        Self::with_path(corrections, Some(path.to_path_buf()))
    }

    /// Loads the dictionary from `path`, or creates the file pre-populated
    /// with all default seed terms and explanatory comments.
    pub fn load_or_create(path: &Path) -> Self {
        if path.exists() {
            Self::load_or_default(path)
        } else {
            let defaults = default_corrections();
            let dict = Self::with_path(defaults, Some(path.to_path_buf()));
            if let Err(e) = dict.save_to_file() {
                tracing::warn!(%e, path = %path.display(), "failed to write initial dictionary.toml");
            } else {
                tracing::info!(path = %path.display(), "created initial dictionary.toml with default rules");
            }
            dict
        }
    }

    /// Adds or updates a correction rule and recompiles the Aho-Corasick matcher.
    pub fn add_rule(&mut self, from: String, to: String, category: Option<String>) {
        let from = from.trim().to_string();
        let to = to.trim().to_string();
        if from.is_empty() || from == to {
            return;
        }

        // Replace existing rule if present, otherwise append
        if let Some(existing) = self.rules.iter_mut().find(|r| r.from == from) {
            existing.to = to;
            if category.is_some() {
                existing.category = category;
            }
        } else {
            self.rules.push(Correction {
                from,
                to,
                category,
            });
        }

        let (matcher, replacements, clean_rules) = Self::compile_rules(&self.rules);
        self.matcher = matcher;
        self.replacements = replacements;
        self.rules = clean_rules;
    }

    /// Removes a rule by index and recompiles the matcher.
    pub fn remove_rule(&mut self, index: usize) -> Option<Correction> {
        if index >= self.rules.len() {
            return None;
        }
        let removed = self.rules.remove(index);
        let (matcher, replacements, clean_rules) = Self::compile_rules(&self.rules);
        self.matcher = matcher;
        self.replacements = replacements;
        self.rules = clean_rules;
        Some(removed)
    }

    /// Removes a rule by its `from` pattern and recompiles the matcher.
    pub fn remove_by_from(&mut self, from: &str) -> bool {
        let from = from.trim();
        if let Some(pos) = self.rules.iter().position(|r| r.from == from) {
            self.remove_rule(pos);
            true
        } else {
            false
        }
    }

    /// Persists current dictionary rules to the file path.
    pub fn save_to_file(&self) -> anyhow::Result<()> {
        let Some(path) = &self.file_path else {
            anyhow::bail!("cannot save dictionary: no file path configured");
        };

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file_content = DictionaryFile {
            corrections: self.rules.clone(),
        };

        let toml_str = toml::to_string_pretty(&file_content)?;
        let header = "# ==============================================================================\n\
                      # دیکشنری کلمات تخصصی — OmniType FreePTT\n\
                      # شما می‌توانید کلمات تخصصی، مهندسی یا برنامه‌نویسی مدنظر خود را در این فایل اضافه کنید.\n\
                      #\n\
                      # فرمت تعریف هر قانون:\n\
                      # [[corrections]]\n\
                      # from = \"کلمه شنیده شده توسط هوش مصنوعی\"\n\
                      # to   = \"معادل صحیح و تخصصی\"\n\
                      # category = \"دسته‌بندی (اختیاری)\"\n\
                      # ==============================================================================\n\n";

        let complete_content = format!("{header}{toml_str}");
        std::fs::write(path, complete_content)?;
        tracing::info!(path = %path.display(), rules = self.rules.len(), "dictionary saved to disk");
        Ok(())
    }

    /// Reloads rules from disk and recompiles the matcher.
    pub fn reload_from_file(&mut self) -> anyhow::Result<()> {
        let Some(path) = &self.file_path else {
            anyhow::bail!("cannot reload dictionary: no file path configured");
        };

        let content = std::fs::read_to_string(path)?;
        let file = toml::from_str::<DictionaryFile>(&content)?;
        let (matcher, replacements, clean_rules) = Self::compile_rules(&file.corrections);
        self.matcher = matcher;
        self.replacements = replacements;
        self.rules = clean_rules;
        tracing::info!(path = %path.display(), rules = self.rules.len(), "dictionary reloaded from disk");
        Ok(())
    }

    /// Returns a slice of all active correction rules.
    pub fn rules(&self) -> &[Correction] {
        &self.rules
    }

    /// Returns the file path if configured.
    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    /// Sets or updates the file path for this dictionary.
    pub fn set_file_path(&mut self, path: PathBuf) {
        self.file_path = Some(path);
    }

    /// Applies all corrections. Replacements are plain text (no capture
    /// groups), so a single ordered pass with leftmost-longest semantics is
    /// sufficient.
    pub fn correct(&self, text: &str) -> String {
        if self.replacements.is_empty() || text.is_empty() {
            return text.to_string();
        }
        self.matcher
            .replace_all(text, &self.replacements)
    }

    /// Number of active rules.
    pub fn len(&self) -> usize {
        self.replacements.len()
    }

    /// Whether the dictionary has no rules.
    pub fn is_empty(&self) -> bool {
        self.replacements.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DictionaryFile {
    #[serde(default)]
    corrections: Vec<Correction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrects_python_mishearing() {
        let d = Dictionary::with_defaults();
        assert_eq!(d.correct("من با پاتون کار می‌کنم"), "من با پایتون کار می‌کنم");
    }

    #[test]
    fn corrects_split_terms() {
        let d = Dictionary::with_defaults();
        assert_eq!(d.correct("جاوا اسکریپت خوب است"), "جاوااسکریپت خوب است");
    }

    #[test]
    fn leaves_normal_text_untouched() {
        let d = Dictionary::with_defaults();
        let text = "امروز هوا خیلی خوب بود";
        assert_eq!(d.correct(text), text);
    }

    #[test]
    fn multiple_replacements_in_one_pass() {
        let d = Dictionary::with_defaults();
        assert_eq!(
            d.correct("اپی آی با داکر"),
            "API با Docker"
        );
    }

    #[test]
    fn user_file_merges_with_defaults() {
        let dir = std::env::temp_dir().join("voice-ptt-dict-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("dict.toml");
        std::fs::write(
            &file,
            "[[corrections]]\nfrom = \"فلان\"\nto = \"فلان‌چیز\"\n",
        )
        .unwrap();

        let d = Dictionary::load_or_default(&file);
        assert!(d.len() > 1, "user rules should merge with defaults");
        assert_eq!(d.correct("این فلان است"), "این فلان‌چیز است");
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let d = Dictionary::load_or_default(Path::new("Z:/no/such/file.toml"));
        assert!(!d.is_empty());
    }

    #[test]
    fn self_rules_are_dropped() {
        let d = Dictionary::new(vec![Correction {
            from: "سرور".into(),
            to: "سرور".into(),
            category: None,
        }]);
        assert!(d.is_empty(), "identity rules must be dropped");
    }

    #[test]
    fn add_and_remove_rule_updates_pipeline() {
        let mut d = Dictionary::with_defaults();
        d.add_rule("کوئری".into(), "Query".into(), Some("دیتابیس".into()));
        assert_eq!(d.correct("این کوئری سریع است"), "این Query سریع است");

        assert!(d.remove_by_from("کوئری"));
        assert_eq!(d.correct("این کوئری سریع است"), "این کوئری سریع است");
    }

    #[test]
    fn specialized_industrial_terms_correction() {
        let d = Dictionary::with_defaults();
        // AITCO petroleum & mechanical terms
        assert_eq!(
            d.correct("سفارش خرید گاسکت و فلانج و بال ولو ثبت شد"),
            "سفارش خرید گسکت و فلنج و Ball Valve ثبت شد"
        );
        assert_eq!(
            d.correct("مدارک پی اند آی دی و ام تی او و آر اف کیو تایید شدند"),
            "مدارک P&ID و MTO و RFQ تایید شدند"
        );
        assert_eq!(
            d.correct("قطعه از جنس اینکونل با دستگاه سی ان سی تراش خورد"),
            "قطعه از جنس Inconel با دستگاه CNC تراش خورد"
        );
    }

    #[test]
    fn load_or_create_persists_new_file() {
        let dir = std::env::temp_dir().join("voice-ptt-dict-test-create");
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("dict_created.toml");

        let d = Dictionary::load_or_create(&file);
        assert!(file.exists());
        assert!(d.len() > 50);

        let d2 = Dictionary::load_or_default(&file);
        assert_eq!(d.len(), d2.len());
    }
}
