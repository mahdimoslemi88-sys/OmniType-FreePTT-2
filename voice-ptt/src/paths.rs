//! Stable on-disk locations for models and assets.
//!
//! Why not just the working directory? When the exe is launched by
//! double-click, the working directory is the exe folder — but **shortcuts
//! with a custom "Start in", Startup entries, or other launchers** can set it
//! anywhere (even `C:\Windows\System32`), and the model would "not be found".
//!
//! Resolution order (first match wins):
//! 1. `<exe dir>\models` — stable across launch methods,
//! 2. `.\models` — preserves the dev / `cargo run` workflow,
//! 3. `%APPDATA%\voice-ptt\models` — per-user fallback.
//!
//! Fresh install (no directory exists anywhere): the first *creatable* of
//! `.\models` → `<exe dir>\models` → `%APPDATA%\voice-ptt\models` is used, so
//! the location stays stable across launches. (cwd-first keeps dev downloads
//! out of `target/`, which `cargo clean` would wipe.)
//!
//! The same logic applies to the `assets` directory (Silero VAD model).

use std::path::PathBuf;

use crate::config::settings::dirs_or_cwd;

/// Resolves (and if necessary creates) the whisper models directory.
pub fn resolve_models_dir() -> PathBuf {
    resolve_dir("models")
}

/// Resolves (and if necessary creates) the assets directory.
pub fn resolve_assets_dir() -> PathBuf {
    resolve_dir("assets")
}

/// Resolves the dictionary.toml file location with portable exe-first priority.
pub fn resolve_dictionary_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("dictionary.toml");
            if p.is_file() {
                return p;
            }
        }
    }
    let p = PathBuf::from("dictionary.toml");
    if p.is_file() {
        return p;
    }
    let p = dirs_or_cwd().join("dictionary.toml");
    if p.is_file() {
        return p;
    }

    // Default target for fresh creation: exe dir if writable, else AppData
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("dictionary.toml");
            if std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&p).is_ok() {
                return p;
            }
        }
    }
    let app_data = dirs_or_cwd();
    let _ = std::fs::create_dir_all(&app_data);
    app_data.join("dictionary.toml")
}

/// Resolves the config.toml file location with portable exe-first priority.
pub fn resolve_config_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("config.toml");
            if p.is_file() {
                return p;
            }
        }
    }
    let p = PathBuf::from("config.toml");
    if p.is_file() {
        return p;
    }
    let p = dirs_or_cwd().join("config.toml");
    if p.is_file() {
        return p;
    }

    // Default target for fresh creation: exe dir if writable, else AppData
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("config.toml");
            if std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&p).is_ok() {
                return p;
            }
        }
    }
    let app_data = dirs_or_cwd();
    let _ = std::fs::create_dir_all(&app_data);
    app_data.join("config.toml")
}

/// Resolves the cloud_usage.json file location with portable exe-first priority.
pub fn resolve_usage_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("cloud_usage.json");
            if p.is_file() {
                return p;
            }
        }
    }
    let p = PathBuf::from("cloud_usage.json");
    if p.is_file() {
        return p;
    }
    dirs_or_cwd().join("cloud_usage.json")
}


fn resolve_dir(dir_name: &str) -> PathBuf {
    // 1-3: reuse an existing directory (stable across launches).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(dir_name);
            if p.is_dir() {
                return p;
            }
        }
    }
    let p = PathBuf::from(dir_name);
    if p.is_dir() {
        return p;
    }
    let p = dirs_or_cwd().join(dir_name);
    if p.is_dir() {
        return p;
    }

    // Fresh install: create the first writable candidate, cwd first.
    let p = PathBuf::from(dir_name);
    if std::fs::create_dir_all(&p).is_ok() {
        return p;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(dir_name);
            if std::fs::create_dir_all(&p).is_ok() {
                return p;
            }
        }
    }
    let p = dirs_or_cwd().join(dir_name);
    let _ = std::fs::create_dir_all(&p);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_is_stable_across_calls() {
        let a = resolve_models_dir();
        let b = resolve_models_dir();
        assert_eq!(a, b, "model dir must not move between launches");
    }

    #[test]
    fn resolved_dir_exists_after_call() {
        let dir = resolve_models_dir();
        assert!(dir.is_dir(), "resolver must create the dir if missing");
        let dir = resolve_assets_dir();
        assert!(dir.is_dir());
    }

    #[test]
    fn models_and_assets_are_distinct() {
        assert_ne!(resolve_models_dir(), resolve_assets_dir());
    }
}
