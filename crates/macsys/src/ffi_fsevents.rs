//! An FSEvents stream over a dispatch queue (ADR-GRP-010, Enmienda 2026-10-08), one call per
//! `unsafe` block, behind [`Raw`]. Nothing here is public outside the crate.
//!
//! Lifetime of the callback context: the stream is given a raw pointer to a boxed [`Context`]
//! with no retain or release callbacks, so the stream never owns it. [`Raw`] frees it only
//! after the stream is stopped and invalidated and its serial queue has run a barrier block,
//! which is after the last callback ran: no callback sees a freed context.

use std::ffi::{CStr, CString, c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::fsevents::{Event, Handler, StreamError};

type CFAllocatorRef = *const c_void;
type CFArrayRef = *const c_void;
type CFStringRef = *const c_void;
type FSEventStreamRef = *mut c_void;
type DispatchQueue = *mut c_void;

/// `kCFStringEncodingUTF8`.
const UTF8: u32 = 0x0800_0100;
/// `kFSEventStreamCreateFlagNoDefer | kFSEventStreamCreateFlagFileEvents`: the flags of
/// `notify` 8.2, so the events are the same.
const STREAM_FLAGS: u32 = 0x02 | 0x10;
/// `kFSEventStreamEventIdSinceNow`.
pub(crate) const SINCE_NOW: u64 = u64::MAX;

/// Opaque `CFArrayCallBacks`: only its address is taken.
#[repr(C)]
struct CFArrayCallBacks {
    _opaque: [u8; 0],
}

/// `FSEventStreamContext`.
#[repr(C)]
struct StreamContext {
    version: isize,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
}

type RawCallback = extern "C" fn(
    stream: FSEventStreamRef,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    event_ids: *const u64,
);

#[allow(clippy::duplicated_attributes)]
#[link(name = "CoreServices", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    #[allow(non_upper_case_globals)]
    static kCFTypeArrayCallBacks: CFArrayCallBacks;
    fn CFArrayCreate(
        allocator: CFAllocatorRef,
        values: *const *const c_void,
        count: isize,
        callbacks: *const CFArrayCallBacks,
    ) -> CFArrayRef;
    fn CFStringCreateWithBytes(
        allocator: CFAllocatorRef,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        is_external: u8,
    ) -> CFStringRef;
    fn CFRelease(object: *const c_void);

    fn FSEventsGetCurrentEventId() -> u64;
    fn FSEventStreamCreate(
        allocator: CFAllocatorRef,
        callback: RawCallback,
        context: *const StreamContext,
        paths: CFArrayRef,
        since_when: u64,
        latency: f64,
        flags: u32,
    ) -> FSEventStreamRef;
    fn FSEventStreamSetExclusionPaths(stream: FSEventStreamRef, paths: CFArrayRef) -> u8;
    fn FSEventStreamSetDispatchQueue(stream: FSEventStreamRef, queue: DispatchQueue);
    fn FSEventStreamStart(stream: FSEventStreamRef) -> u8;
    fn FSEventStreamFlushSync(stream: FSEventStreamRef);
    fn FSEventStreamStop(stream: FSEventStreamRef);
    fn FSEventStreamInvalidate(stream: FSEventStreamRef);
    fn FSEventStreamRelease(stream: FSEventStreamRef);

    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> DispatchQueue;
    fn dispatch_sync_f(
        queue: DispatchQueue,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    fn dispatch_release(object: DispatchQueue);
}

/// What the callback reaches through its `info` pointer.
struct Context {
    handler: Handler,
    /// The highest event id delivered, or the id the stream started from.
    last_id: Arc<AtomicU64>,
}

/// A `CFArray` of `CFString`s, released on drop.
struct StringArray(CFArrayRef);

impl StringArray {
    fn new(paths: &[&str]) -> Option<Self> {
        let mut strings: Vec<*const c_void> = Vec::with_capacity(paths.len());
        let mut ok = true;
        for p in paths {
            let len = isize::try_from(p.len()).ok();
            // SAFETY: `p` is a live `&str` of `len` bytes; the call copies them and returns an
            // owned string or null.
            let s = len.map_or(null(), |len| unsafe {
                CFStringCreateWithBytes(null(), p.as_ptr(), len, UTF8, 0)
            });
            if s.is_null() {
                ok = false;
                break;
            }
            strings.push(s);
        }
        let count = isize::try_from(strings.len()).ok();
        let array = match count {
            Some(count) if ok => {
                // SAFETY: `strings` holds `count` valid `CFString`s; the array retains them
                // (`kCFTypeArrayCallBacks`), whose address is that of a static.
                unsafe {
                    CFArrayCreate(
                        null(),
                        strings.as_ptr(),
                        count,
                        &raw const kCFTypeArrayCallBacks,
                    )
                }
            }
            _ => null(),
        };
        for s in strings {
            // SAFETY: an owned string created above; the array holds its own reference.
            unsafe { CFRelease(s) };
        }
        (!array.is_null()).then_some(Self(array))
    }
}

impl Drop for StringArray {
    fn drop(&mut self) {
        // SAFETY: an owned array, never used after this.
        unsafe { CFRelease(self.0) };
    }
}

/// A running stream.
pub(crate) struct Raw {
    stream: FSEventStreamRef,
    queue: DispatchQueue,
    context: *mut Context,
    last_id: Arc<AtomicU64>,
}

// SAFETY: the stream and the queue are only used through thread-safe CoreServices and libdispatch
// calls, and the context is only read by the callback, which `Drop` waits for before freeing it.
unsafe impl Send for Raw {}
// SAFETY: as above; `&Raw` only reads `last_id` and calls `FSEventStreamFlushSync`, which the
// framework allows from any thread.
unsafe impl Sync for Raw {}

extern "C" fn trampoline(
    _stream: FSEventStreamRef,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    event_ids: *const u64,
) {
    // A panic must not unwind into CoreServices.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `info` is the `Context` `create` passed, alive until `Raw::drop` drained the
        // queue this runs on.
        let ctx = unsafe { &*info.cast::<Context>() };
        let paths = event_paths.cast::<*const c_char>();
        // SAFETY: FSEvents passes `num_events` valid entries in each of the three arrays.
        let paths = unsafe { std::slice::from_raw_parts(paths, num_events) };
        // SAFETY: as above.
        let flags = unsafe { std::slice::from_raw_parts(event_flags, num_events) };
        // SAFETY: as above.
        let ids = unsafe { std::slice::from_raw_parts(event_ids, num_events) };
        let mut events = Vec::with_capacity(num_events);
        for i in 0..num_events {
            // SAFETY: each path is a NUL-terminated string that outlives this callback (no
            // `UseCFTypes` flag).
            let path = unsafe { CStr::from_ptr(paths[i]) };
            events.push(Event {
                path: Path::new(std::ffi::OsStr::from_bytes(path.to_bytes())),
                flags: flags[i],
                id: ids[i],
            });
            if ids[i] != 0 {
                ctx.last_id.fetch_max(ids[i], Ordering::Relaxed);
            }
        }
        (ctx.handler)(&events);
    }));
}

extern "C" fn barrier(_: *mut c_void) {}

/// Creates and starts a stream over `root` that skips `exclusions`.
pub(crate) fn create(
    root: &str,
    exclusions: &[&str],
    since_when: u64,
    handler: Handler,
) -> Result<Raw, StreamError> {
    let roots = StringArray::new(&[root]).ok_or(StreamError::InvalidPath)?;
    let excluded = if exclusions.is_empty() {
        None
    } else {
        Some(StringArray::new(exclusions).ok_or(StreamError::InvalidPath)?)
    };
    let initial = if since_when == SINCE_NOW {
        // SAFETY: a plain query of the current event id.
        unsafe { FSEventsGetCurrentEventId() }
    } else {
        since_when
    };
    let last_id = Arc::new(AtomicU64::new(initial));
    let context = Box::into_raw(Box::new(Context {
        handler,
        last_id: Arc::clone(&last_id),
    }));
    let info = StreamContext {
        version: 0,
        info: context.cast(),
        retain: null(),
        release: null(),
        copy_description: null(),
    };
    // SAFETY: `info` and `roots` are live for the call, which copies the context struct and
    // retains the array; the callback has the signature `FSEventStreamCallback` declares.
    let stream = unsafe {
        FSEventStreamCreate(
            null(),
            trampoline,
            &info,
            roots.0,
            since_when,
            0.0,
            STREAM_FLAGS,
        )
    };
    let free = |context: *mut Context| {
        // SAFETY: created by `Box::into_raw` above; the stream never started, or was released.
        drop(unsafe { Box::from_raw(context) });
    };
    if stream.is_null() {
        free(context);
        return Err(StreamError::Create);
    }
    if let Some(excluded) = &excluded {
        // SAFETY: a live, not yet started stream and a live array (copied by the call).
        let ok = unsafe { FSEventStreamSetExclusionPaths(stream, excluded.0) };
        if ok == 0 {
            // SAFETY: an owned stream, never used after this.
            unsafe { FSEventStreamRelease(stream) };
            free(context);
            return Err(StreamError::Exclusions);
        }
    }
    let label = CString::new("gitraptor.fsevents").unwrap_or_default();
    // SAFETY: `label` is a live C string, copied by the call; a null attribute is a serial queue.
    let queue = unsafe { dispatch_queue_create(label.as_ptr(), null()) };
    // SAFETY: a live, not yet started stream and a live queue, which the stream retains.
    unsafe { FSEventStreamSetDispatchQueue(stream, queue) };
    // SAFETY: a live stream with a queue.
    let started = unsafe { FSEventStreamStart(stream) };
    let raw = Raw {
        stream,
        queue,
        context,
        last_id,
    };
    if started == 0 {
        drop(raw);
        return Err(StreamError::Start);
    }
    Ok(raw)
}

impl Raw {
    /// The highest event id delivered (or the one the stream started from).
    pub(crate) fn last_event_id(&self) -> u64 {
        self.last_id.load(Ordering::Relaxed)
    }

    /// Delivers every event the OS already has, then returns.
    pub(crate) fn flush_sync(&self) {
        // SAFETY: a live, started stream; allowed from any thread but its queue.
        unsafe { FSEventStreamFlushSync(self.stream) };
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // SAFETY: a live stream owned by `self`.
        unsafe { FSEventStreamStop(self.stream) };
        // SAFETY: as above; it also detaches the stream from its queue.
        unsafe { FSEventStreamInvalidate(self.stream) };
        // A barrier on the serial queue: every callback queued before it has returned.
        // SAFETY: a live queue and a function that ignores its null context.
        unsafe { dispatch_sync_f(self.queue, null_mut(), barrier) };
        // SAFETY: an owned stream, never used after this.
        unsafe { FSEventStreamRelease(self.stream) };
        // SAFETY: an owned queue, never used after this.
        unsafe { dispatch_release(self.queue) };
        // SAFETY: created by `Box::into_raw` in `create`; no callback can run any more.
        drop(unsafe { Box::from_raw(self.context) });
    }
}
