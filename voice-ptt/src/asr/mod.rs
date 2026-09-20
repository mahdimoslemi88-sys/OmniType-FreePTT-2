//! ASR layer: engine abstraction, whisper.cpp engine, cloud engine,
//! model downloader and router.

pub mod cloud;
pub mod downloader;
pub mod engine;
pub mod google;
pub mod quota;
pub mod router;
pub mod whisper;

pub use cloud::CloudEngine;
pub use engine::{AsrEngine, AsrHealth, AudioUtterance};
pub use google::GoogleEngine;
pub use quota::DailyQuota;
pub use router::AsrRouter;
pub use whisper::{WhisperEngine, WhisperOptions};
