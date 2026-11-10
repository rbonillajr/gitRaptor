//! Synthetic input (E10/E14) and a listen-only event tap that reads
//! `kCGEventSourceUnixProcessID` (H13).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFMachPort, CFRunLoop, CGPoint, kCFRunLoopDefaultMode};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventMask, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType, CGMouseButton,
    CGPreflightListenEventAccess, CGPreflightPostEventAccess,
};
use serde_json::{Value, json};

pub fn access() -> Value {
    json!({"post_event_access": CGPreflightPostEventAccess(), "listen_event_access": CGPreflightListenEventAccess()})
}

fn source(state: &str) -> Option<objc2_core_foundation::CFRetained<CGEventSource>> {
    let id = match state {
        "private" => CGEventSourceStateID::Private,
        "hid" => CGEventSourceStateID::HIDSystemState,
        _ => CGEventSourceStateID::CombinedSessionState,
    };
    CGEventSource::new(id)
}

fn tap_loc(loc: &str) -> CGEventTapLocation {
    if loc == "session" {
        CGEventTapLocation::SessionEventTap
    } else {
        CGEventTapLocation::HIDEventTap
    }
}

pub fn key(keycode: u16, state: &str, loc: &str) {
    let src = source(state);
    for down in [true, false] {
        let ev = CGEvent::new_keyboard_event(src.as_deref(), keycode, down);
        CGEvent::post(tap_loc(loc), ev.as_deref());
        std::thread::sleep(Duration::from_millis(15));
    }
}

pub fn type_text(text: &str, state: &str, loc: &str) {
    let src = source(state);
    for ch in text.encode_utf16() {
        for down in [true, false] {
            let ev = CGEvent::new_keyboard_event(src.as_deref(), 0, down);
            let buf = [ch];
            // SAFETY: `buf` is a valid UTF-16 buffer of length 1 for the call's duration.
            unsafe { CGEvent::keyboard_set_unicode_string(ev.as_deref(), 1, buf.as_ptr()) };
            CGEvent::post(tap_loc(loc), ev.as_deref());
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}

pub fn click(x: f64, y: f64, state: &str, loc: &str) {
    let src = source(state);
    let p = CGPoint { x, y };
    for t in [CGEventType::LeftMouseDown, CGEventType::LeftMouseUp] {
        let ev = CGEvent::new_mouse_event(src.as_deref(), t, p, CGMouseButton::Left);
        CGEvent::post(tap_loc(loc), ev.as_deref());
        std::thread::sleep(Duration::from_millis(30));
    }
}

struct TapState {
    start: Instant,
    events: Vec<Value>,
}

unsafe extern "C-unwind" fn tap_cb(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    user: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `user` is the `&mut TapState` passed to tap_create, alive for the run loop.
    let st = unsafe { &mut *(user as *mut TapState) };
    // SAFETY: the event is valid for the duration of the callback.
    let ev = unsafe { event.as_ref() };
    let pid = CGEvent::integer_value_field(Some(ev), CGEventField::EventSourceUnixProcessID);
    let state_id = CGEvent::integer_value_field(Some(ev), CGEventField::EventSourceStateID);
    let user_data = CGEvent::integer_value_field(Some(ev), CGEventField::EventSourceUserData);
    if st.events.len() < 200 {
        st.events.push(json!({
            "t_ms": st.start.elapsed().as_millis() as u64,
            "type": ty.0,
            "source_pid": pid,
            "source_state_id": state_id,
            "source_user_data": user_data,
        }));
    }
    event.as_ptr()
}

/// Listens (no modification) to key and mouse events for `secs` seconds.
pub fn tap(secs: f64) -> Value {
    let mask: CGEventMask = (1 << CGEventType::KeyDown.0)
        | (1 << CGEventType::LeftMouseDown.0)
        | (1 << CGEventType::LeftMouseUp.0);
    let mut st = Box::new(TapState {
        start: Instant::now(),
        events: vec![],
    });
    // SAFETY: callback has the right signature; user_info is a Box we keep alive
    // until after the run loop returns and the port is invalidated.
    let port = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::HIDEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            Some(tap_cb),
            (&mut *st) as *mut TapState as *mut c_void,
        )
    };
    let Some(port) = port else {
        return json!({"error": "CGEventTapCreate returned null (no Input Monitoring/Accessibility permission?)", "access": access()});
    };
    let src = CFMachPort::new_run_loop_source(None, Some(&port), 0);
    let rl = CFRunLoop::current().expect("run loop");
    // SAFETY: kCFRunLoopDefaultMode is a valid static CFString.
    let mode = unsafe { kCFRunLoopDefaultMode };
    rl.add_source(src.as_deref(), mode);
    CFRunLoop::run_in_mode(mode, secs, false);
    CFMachPort::invalidate(&port);
    json!({"secs": secs, "events": st.events, "access": access()})
}
