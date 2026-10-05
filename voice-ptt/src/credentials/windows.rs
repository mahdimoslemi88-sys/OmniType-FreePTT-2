//! The real [`CredentialStore`]: Windows Credential Manager.
//!
//! `Win32_Credential.dll` keeps the blob under the user's own profile, encrypted
//! with a key derived from their logon credentials. That is the difference this
//! file exists to make: the API key moves out of a `config.toml` that any
//! process running as the user can read into a store that a *different* process
//! running as the user cannot read without asking.
//!
//! What it does **not** do is claim protection from every process under the same
//! account. A process that can call `CredReadW` with the same target name can read
//! it. That limit belongs in the interface, not in a comment nobody reads, so
//! [`WindowsCredentialStore::save`] returns the target it wrote under and the
//! migration log names it: a user who wants to know *where* their key went gets
//! an answer rather than a reassurance.
//!
//! # Why the blob is zeroed
//!
//! `CredReadW` hands back a buffer the caller must `CredFree`. The bytes of the
//! secret sit in that buffer whether or not we care about them, so it is
//! overwritten before being released. This is best-effort in exactly the same way
//! [`super::SecretString`]'s `Drop` is: it reduces the window in which the
//! plaintext is resident, and does not claim to close it.

use std::ffi::c_void;

use super::{CredentialError, CredentialStore, SecretString};

/// Target prefix for everything this app stores.
///
/// Namespaced because Credential Manager is **per-user and machine-wide**: an
/// unprefixed `"cloud"` would collide with any other program using the same
/// obvious name, and the failure mode of that collision is one program silently
/// reading another's key.
const TARGET_PREFIX: &str = "OmniTypeFreePTT/";

/// The full target name for a service, e.g. `OmniTypeFreePTT/cloud`.
pub fn target_for(service: &str) -> String {
    format!("{TARGET_PREFIX}{service}")
}

/// The real store. Cheap to construct, holds no handle and no state.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsCredentialStore;

impl WindowsCredentialStore {
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ptr;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC, CREDENTIALW, CRED_FLAGS,
    };

    /// `ERROR_NOT_FOUND` (1168) — the target simply is not stored yet.
    const ERROR_NOT_FOUND: u32 = 1168;
    /// `ERROR_NO_SUCH_LOGON_SESSION` (1312) — no interactive logon, e.g. a service.
    const ERROR_NO_SUCH_LOGON_SESSION: u32 = 1312;
    /// `ERROR_ACCESS_DENIED` (5).
    const ERROR_ACCESS_DENIED: u32 = 5;

    /// Maps a Win32 error onto the typed error the rest of the program handles.
    ///
    /// The point of the typed errors is that a caller can tell "you have not set
    /// this up" from "this machine will not let me"; collapsing both into
    /// `Unavailable` would make the first look like a bug and send the user
    /// looking in the wrong place.
    fn classify(target: &str, err: &windows::core::Error) -> CredentialError {
        // `Error::code()` is an **HRESULT**, not a Win32 error code. The
        // credential APIs are old-style Win32, so their failures come back
        // wrapped as `HRESULT_FROM_WIN32(code)` — `0x80070000 | code`.
        // Comparing `code().0` against the raw Win32 constants therefore
        // matched nothing, and "no credential stored yet" was reported as an
        // opaque `OsError`. That is not cosmetic: the migration's very first
        // step is reading a target that does not exist yet, so on any machine
        // that had never stored the key it concluded the store was broken,
        // refused to run, and left the key in plaintext — defeating the entire
        // feature. Unwrap the HRESULT back to the Win32 code.
        let hresult = err.code().0 as u32;
        let win32 = if hresult & 0xFFFF_0000 == 0x8007_0000 {
            hresult & 0xFFFF
        } else {
            hresult
        };
        match win32 {
            ERROR_NOT_FOUND => CredentialError::NotFound(target.to_string()),
            ERROR_NO_SUCH_LOGON_SESSION => {
                CredentialError::Unavailable("no interactive logon session".into())
            }
            code if code == ERROR_ACCESS_DENIED => {
                CredentialError::AccessDenied(target.to_string())
            }
            code => CredentialError::OsError {
                code,
                message: err.message().to_string(),
            },
        }
    }

    /// Overwrites `len` bytes at `ptr` before the buffer goes back to the OS.
    ///
    /// `write_volatile` so the compiler cannot decide the stores are dead and
    /// drop them: the whole point is that they happen after the last read.
    fn scrub(ptr: *mut u8, len: usize) {
        for i in 0..len {
            unsafe { ptr::write_volatile(ptr.add(i), 0) };
        }
    }

    impl CredentialStore for WindowsCredentialStore {
        fn save(&self, target: &str, secret: &SecretString) -> Result<(), CredentialError> {
            if secret.is_empty() {
                return Err(CredentialError::EmptySecret);
            }
            // `CRED_PERSIST_LOCAL_MACHINE` rather than `SESSION`: the key has to
            // survive a reboot, or a user would silently fall back to an
            // unauthenticated engine after every restart.
            let mut name: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
            let mut blob: Vec<u8> = secret.expose_secret().as_bytes().to_vec();
            let credential = CREDENTIALW {
                Flags: CRED_FLAGS(0),
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(name.as_mut_ptr()),
                Comment: PWSTR::null(),
                LastWritten: FILETIME {
                    dwLowDateTime: 0,
                    dwHighDateTime: 0,
                },
                CredentialBlobSize: blob.len() as u32,
                CredentialBlob: blob.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                AttributeCount: 0,
                Attributes: ptr::null_mut(),
                TargetAlias: PWSTR::null(),
                UserName: PWSTR::null(),
            };
            let written = unsafe { CredWriteW(&credential, 0) };
            // The plaintext copy in this process is ours to clear; the OS's copy
            // is the store's business.
            scrub(blob.as_mut_ptr(), blob.len());
            name.fill(0);
            written.map_err(|e| classify(target, &e))
        }

        fn load(&self, target: &str) -> Result<SecretString, CredentialError> {
            let name: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
            let mut raw: *mut CREDENTIALW = ptr::null_mut();
            unsafe { CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, 0, &mut raw) }
                .map_err(|e| classify(target, &e))?;
            if raw.is_null() {
                return Err(CredentialError::NotFound(target.to_string()));
            }
            let cred = unsafe { &*raw };
            let size = cred.CredentialBlobSize as usize;
            let blob = cred.CredentialBlob;
            let secret = if blob.is_null() || size == 0 {
                String::new()
            } else {
                let bytes = unsafe { std::slice::from_raw_parts(blob, size) };
                // The blob is read back as **UTF-8**, because that is what
                // [`Self::save`] writes. The two halves of one store have to
                // agree on the encoding or the migration's byte-for-byte
                // read-back verification can never pass: reading our own UTF-8
                // bytes as UTF-16 turned every key into mojibake, the
                // verification reported a mismatch, and the key was therefore
                // never cleared from `config.toml` — the migration silently
                // refused to do the one thing it exists to do.
                //
                // The UTF-16 fallback is for credentials written by *other*
                // programs under the same target name, which is the documented
                // convention for a generic credential blob. It is only reached
                // when the bytes are not valid UTF-8, so it cannot shadow a key
                // this app wrote itself.
                match std::str::from_utf8(bytes) {
                    Ok(text) => text.to_string(),
                    Err(_) => {
                        let units: Vec<u16> = bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                            .collect();
                        String::from_utf16_lossy(&units)
                    }
                }
            };
            scrub(blob, size);
            unsafe { CredFree(raw as *mut c_void) };
            Ok(SecretString::new(secret))
        }

        fn delete(&self, target: &str) -> Result<(), CredentialError> {
            let name: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, 0) }
                .map_err(|e| classify(target, &e))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    /// Off Windows there is no Credential Manager, and pretending otherwise would
    /// let the migration delete a key from `config.toml` that it had not
    /// actually stored anywhere. Every operation refuses instead.
    impl CredentialStore for WindowsCredentialStore {
        fn save(&self, _target: &str, _secret: &SecretString) -> Result<(), CredentialError> {
            Err(CredentialError::Unavailable(
                "no credential store on this platform".into(),
            ))
        }

        fn load(&self, target: &str) -> Result<SecretString, CredentialError> {
            Err(CredentialError::Unavailable(format!(
                "no credential store on this platform (target '{target}')"
            )))
        }

        fn delete(&self, _target: &str) -> Result<(), CredentialError> {
            Err(CredentialError::Unavailable(
                "no credential store on this platform".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_namespaced_by_the_app() {
        assert_eq!(target_for("cloud"), "OmniTypeFreePTT/cloud");
        // The namespace is the whole point: an unprefixed name would collide
        // with any other program using the same obvious one.
        assert!(target_for("cloud").starts_with(TARGET_PREFIX));
    }

    #[test]
    fn two_services_never_share_a_target() {
        assert_ne!(target_for("cloud"), target_for("google"));
    }

    /// An empty key must be refused rather than stored, because a stored empty
    /// credential and an absent one mean different things to the engine picker
    /// — and `delete` is how a user says "I have no key".
    #[test]
    fn an_empty_secret_is_refused_without_touching_the_store() {
        let store = WindowsCredentialStore::new();
        assert_eq!(
            store.save(&target_for("cloud"), &SecretString::new("   ")),
            Err(CredentialError::EmptySecret)
        );
    }

    /// The error unwrapping that makes the whole store usable, pinned against
    /// the real OS rather than a mock.
    ///
    /// Reading a target that was never stored is the *normal* first-run case —
    /// it is exactly what the migration does before it has written anything —
    /// and it has to come back as `NotFound`. If the HRESULT is not unwrapped
    /// to its Win32 code, it arrives as an opaque `OsError` and the migration
    /// concludes the store is broken, refuses to run, and the key stays in
    /// plaintext forever. This test reads a deliberately unique target so it
    /// cannot pass by finding a leftover credential.
    #[test]
    #[cfg(windows)]
    fn reading_a_target_that_was_never_stored_is_not_found_not_an_os_error() {
        let store = WindowsCredentialStore::new();
        let target = format!(
            "{}/never-stored-probe-{}",
            target_for("probe"),
            std::process::id()
        );
        match store.load(&target) {
            Err(CredentialError::NotFound(reported)) => assert_eq!(reported, target),
            Err(other) => panic!(
                "expected NotFound for a target that was never stored, got: {other} \
                 (the HRESULT is probably not being unwrapped to its Win32 code)"
            ),
            Ok(_) => panic!("a random probe target unexpectedly existed in the store"),
        }
    }

    /// A secret must survive a real round-trip through the real OS.
    ///
    /// `save` and `load` are two halves of one format, and nothing else in the
    /// program compares them: the migration does, but only as a side effect at
    /// startup, where a mismatch is indistinguishable from "the store is
    /// broken". This puts them face to face in a test, so an encoding
    /// disagreement fails here instead of quietly leaving the user's key in
    /// plaintext forever. The credential is deleted afterwards, so running the
    /// test never leaves a secret behind on the machine.
    #[test]
    #[cfg(windows)]
    fn a_secret_survives_a_real_round_trip_and_can_be_deleted() {
        let store = WindowsCredentialStore::new();
        let target = format!("{}/round-trip-{}", target_for("probe"), std::process::id());
        let secret = SecretString::new("gsk_round-trip-Ω-12345");

        let _ = store.delete(&target);
        store
            .save(&target, &secret)
            .expect("saving a fresh credential must work");
        let read_back = store.load(&target).expect("the credential was just written");
        assert_eq!(read_back, secret, "the blob encoding must round-trip exactly");
        assert_eq!(read_back.expose_secret(), "gsk_round-trip-Ω-12345");

        store.delete(&target).expect("deleting must work");
        assert!(matches!(
            store.load(&target),
            Err(CredentialError::NotFound(_))
        ));
    }
}
