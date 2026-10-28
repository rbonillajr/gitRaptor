//! The class of work of each daemon thread (ADR-GRP-015 § 1; DS-TS-GRP-005 T002): the OS
//! priority of the thread follows the class it declares. macOS applies it; Linux and Windows
//! leave the thread untouched (D4).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;

pub use gitraptor_api::resources::{Pool, PoolClass, WorkClass};

use crate::daemon::{Field, Logger};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
mod unsupported;
#[cfg(not(target_os = "macos"))]
use unsupported as platform;

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
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            // Back to the class the thread had before the stretch, never above it.
            set_current(previous);
        }
    }
}

/// Whether this OS applies classes: `Applied` on macOS, `Unavailable` elsewhere (D4).
pub fn support() -> Support {
    platform::support()
}

/// The class the OS reports for the current thread; `None` where classes are unavailable.
pub fn current() -> Option<WorkClass> {
    platform::current()
}

/// Puts the current thread in `class`. `Unavailable` when the OS does not apply it: the thread
/// keeps the class it inherited and `work_class_unavailable` is logged once per process.
pub fn set_current(class: WorkClass) -> Outcome {
    if platform::set(class) {
        Outcome::Applied
    } else {
        report_unavailable(class);
        Outcome::Unavailable
    }
}

/// Lowers the current thread to `class` until the guard is dropped (D5). Never raises: when
/// `class` is not lower than the current one, or the class of the thread cannot be read (and so
/// cannot be restored), the guard is empty and nothing changes.
pub fn scoped(class: WorkClass) -> ClassGuard {
    let empty = ClassGuard { previous: None };
    let Some(current) = current() else {
        return empty;
    };
    if rank(class) <= rank(current) || set_current(class) != Outcome::Applied {
        return empty;
    }
    ClassGuard {
        previous: Some(current),
    }
}

/// A named thread of `pool` that runs in `class` and is listed by [`pools`] while it lives.
///
/// # Errors
///
/// The error of [`std::thread::Builder::spawn`].
pub fn spawn<F, T>(pool: Pool, class: WorkClass, name: &str, f: F) -> std::io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_unpooled(class, name, move || {
        let _entry = Entry::register(pool, class);
        f()
    })
}

/// A named thread that runs in `class` and belongs to no pool of ADR-GRP-015 § 1.
///
/// # Errors
///
/// The error of [`std::thread::Builder::spawn`].
pub fn spawn_unpooled<F, T>(class: WorkClass, name: &str, f: F) -> std::io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            set_current(class);
            f()
        })
}

/// The pools with a live thread, in the order of [`Pool`], without duplicates.
pub fn pools() -> Vec<PoolClass> {
    let mut live: Vec<PoolClass> = registry()
        .iter()
        .map(|&(_, pool, class)| PoolClass { pool, class })
        .collect();
    live.sort_by_key(|p| (pool_rank(p.pool), rank(p.class)));
    live.dedup();
    live
}

/// Where `work_class_unavailable` goes. The daemon sets it when it starts; before that, and in
/// a process without a daemon, the report waits for the first call made with a logger.
/// `None` takes it back when the daemon stops, so a stopped daemon's log is not kept open.
pub(crate) fn set_logger(logger: Option<Logger>) {
    *LOGGER.lock().unwrap_or_else(PoisonError::into_inner) = logger;
}

static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);
static REPORTED: AtomicBool = AtomicBool::new(false);

fn report_unavailable(class: WorkClass) {
    let guard = LOGGER.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(logger) = guard.as_ref() else {
        return;
    };
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let thread = std::thread::current();
    logger.warn(
        "work_class_unavailable",
        &[
            ("class", Field::Text(class_name(class))),
            ("thread", Field::word(thread.name().unwrap_or("unnamed"))),
        ],
    );
}

/// The live pool threads: a token per thread, its pool and its class.
static REGISTRY: Mutex<Vec<(u64, Pool, WorkClass)>> = Mutex::new(Vec::new());
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(0);

fn registry() -> std::sync::MutexGuard<'static, Vec<(u64, Pool, WorkClass)>> {
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The registry entry of one pool thread; dropped when the thread's closure ends, also while
/// unwinding from a panic, so the pool leaves the registry before `join` returns.
struct Entry(u64);

impl Entry {
    fn register(pool: Pool, class: WorkClass) -> Self {
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        registry().push((token, pool, class));
        Self(token)
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        registry().retain(|&(token, _, _)| token != self.0);
    }
}

/// Higher classes first: a larger rank is a lower class.
fn rank(class: WorkClass) -> u8 {
    match class {
        WorkClass::UserInitiated => 0,
        WorkClass::Default => 1,
        WorkClass::Utility => 2,
        WorkClass::Background => 3,
    }
}

fn pool_rank(pool: Pool) -> u8 {
    match pool {
        Pool::Observer => 0,
        Pool::Executor => 1,
        Pool::TimeMachineWriter => 2,
        Pool::Capture => 3,
        Pool::Maintenance => 4,
        Pool::Predictor => 5,
        Pool::Reconciliation => 6,
    }
}

fn class_name(class: WorkClass) -> &'static str {
    match class {
        WorkClass::UserInitiated => "user-initiated",
        WorkClass::Default => "default",
        WorkClass::Utility => "utility",
        WorkClass::Background => "background",
    }
}
