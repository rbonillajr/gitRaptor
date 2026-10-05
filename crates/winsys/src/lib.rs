//! Windows system calls behind a safe API (ADR-GRP-002).
//!
//! The only crate of the workspace allowed to use `unsafe`, and only inside its private FFI
//! modules: every public module is safe and exposes no pointer or handle. On other OSes only the pure
//! parts (parsing and rules) build, so their tests run everywhere.
#![deny(unsafe_code)]

pub mod acl;

#[cfg(windows)]
#[allow(unsafe_code)]
mod ffi_acl;
