//! US-GRP-002 observer suite, run once per watcher backend (ADR-GRP-010, Enmienda 2026-10-08): the engine's own
//! FSEvents stream and, on macOS, the `notify` fallback (`engine.watcher.backend = "notify"`).
//! Elsewhere the one backend is `notify`, so the suite runs once. The tests are in
//! `suites/watch.rs`.

mod fsevents {
    const BACKEND: gitraptor_core::watch::WatchBackend =
        gitraptor_core::watch::WatchBackend::Fsevents;
    include!("suites/watch.rs");
}

#[cfg(target_os = "macos")]
mod notify_backend {
    const BACKEND: gitraptor_core::watch::WatchBackend =
        gitraptor_core::watch::WatchBackend::Notify;
    include!("suites/watch.rs");
}
