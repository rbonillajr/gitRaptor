//! The class of work of each daemon thread (ADR-GRP-015 § 1; DS-TS-GRP-005 T002): the OS
//! priority of the thread follows the class it declares. macOS applies it; Linux and Windows
//! leave the thread untouched (D4).

use std::thread::JoinHandle;

pub use gitraptor_api::resources::{Pool, PoolClass, WorkClass};

/// Whether this OS applies classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Applied,
    Unavailable,
}

/// What a call did to the current thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    Unavailable,
}

/// Lowers the current thread for a stretch of work and restores the previous class when
/// dropped; empty when nothing was lowered (D5).
#[derive(Debug)]
#[must_use = "the class is restored when the guard is dropped"]
pub struct ClassGuard {
    previous: Option<WorkClass>,
}

impl ClassGuard {
    /// Whether the guard lowered the thread and will restore it.
    pub fn is_active(&self) -> bool {
        self.previous.is_some()
    }
}

impl Drop for ClassGuard {
    fn drop(&mut self) {}
}

pub fn support() -> Support {
    todo!("T002")
}

/// The class the OS reports for the current thread; `None` where classes are unavailable.
pub fn current() -> Option<WorkClass> {
    todo!("T002")
}

pub fn set_current(class: WorkClass) -> Outcome {
    let _ = class;
    todo!("T002")
}

pub fn scoped(class: WorkClass) -> ClassGuard {
    let _ = class;
    todo!("T002")
}

/// A named thread of `pool` that runs in `class` and is listed by [`pools`] while it lives.
pub fn spawn<F, T>(pool: Pool, class: WorkClass, name: &str, f: F) -> std::io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let _ = (pool, class, name, f);
    todo!("T002")
}

/// A named thread that runs in `class` and belongs to no pool of ADR-GRP-015 § 1.
pub fn spawn_unpooled<F, T>(class: WorkClass, name: &str, f: F) -> std::io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let _ = (class, name, f);
    todo!("T002")
}

/// The pools with a live thread, in the order of [`Pool`], without duplicates.
pub fn pools() -> Vec<PoolClass> {
    todo!("T002")
}
