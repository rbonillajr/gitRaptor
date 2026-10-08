//! macOS system calls behind a safe API (ADR-GRP-002, Enmienda 2026-10-07).
//!
//! Like `gitraptor-winsys`, one of the two crates of the workspace allowed to use `unsafe`, and
//! only inside its private FFI modules (`ffi_*`): every public module is safe and exposes no
//! pointer. Each `unsafe` block makes one call and states why it is sound. On other OSes only
//! the pure parts (parsing) build, so their tests run everywhere. `tests/unsafe_boundary.rs`
//! keeps this boundary.
#![deny(unsafe_code)]

pub mod process;

#[cfg(target_os = "macos")]
pub mod fsevents;

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod ffi_fsevents;
#[cfg(all(
    target_os = "macos",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[allow(unsafe_code)]
mod ffi_kinfo;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod ffi_procargs;
