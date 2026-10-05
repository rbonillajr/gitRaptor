//! The Win32 calls GitRaptor needs, behind a safe API (ADR-GRP-005,
//! Enmienda 2026-10-05; ADR-GRP-002, Enmienda 2026-10-05).
//!
//! The workspace forbids `unsafe`. This crate is the one exception, and
//! within it only the private `ffi` module may use it: a small, reviewed
//! surface over `windows-sys`, where every `unsafe` block makes one call and
//! states why it is sound. Nothing outside this crate touches Win32
//! directly. On other platforms it is empty.
//!
//! Rules for adding to it: one documented call per block, handles owned by
//! guards that close them once, buffers sized by the API or by a fixed
//! bound, no pointer kept after the call that filled it, and a test on
//! Windows for every function.

#![deny(unsafe_code)]

#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi;
#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub mod system;
