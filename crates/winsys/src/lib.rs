//! Windows system calls behind a safe API (ADR-GRP-002, Enmienda 2026-10-05).
//!
//! The only crate of the workspace allowed to use `unsafe`, and only inside its private FFI
//! modules (`ffi_*`): every public module is safe and exposes no pointer or handle. Each
//! `unsafe` block makes one call and states why it is sound; handles are owned by guards that
//! close them once. On other OSes only the pure parts (parsing and rules) build, so their tests
//! run everywhere. `tests/unsafe_boundary.rs` keeps this boundary.
#![deny(unsafe_code)]

pub mod acl;
#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
pub mod usage;

#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi_acl;
#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi_handle;
#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi_process;
#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi_usage;
