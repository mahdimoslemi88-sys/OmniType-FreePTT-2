//! Post-processing pipeline: normalization → dictionary correction, plus the
//! chunk-seam stitcher (`seam`) that keeps a long, chunked dictation reading as
//! one continuous piece of text.

pub mod dictionary;
pub mod normalizer;
pub mod seam;

pub use dictionary::Dictionary;
pub use normalizer::Normalizer;
pub use seam::{SeamMerge, SeamOptions, SeamStitcher};

/// Runs the full text post-processing pipeline in order.
pub fn process_text(text: &str, normalizer: &Normalizer, dictionary: &Dictionary) -> String {
    let normalized = normalizer.normalize(text);
    dictionary.correct(&normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_pipeline_normalizes_then_corrects() {
        let n = Normalizer::new();
        let d = Dictionary::with_defaults();
        // Arabic kaf + mishearing in one string.
        let out = process_text("من با پاتون و لینوكس کار میکنم", &n, &d);
        assert_eq!(out, "من با پایتون و لینوکس کار می‌کنم");
    }
}
