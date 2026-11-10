//! Session and window-server introspection (SessionGetInfo, CGSession, window list).
#![allow(unsafe_code)]

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFArray, CFDictionary, CFRetained};
use objc2_core_graphics::{
    CGSessionCopyCurrentDictionary, CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use serde_json::{Value, json};

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SessionGetInfo(session: u32, session_id: *mut u32, attributes: *mut u32) -> i32;
}

const CALLER_SECURITY_SESSION: u32 = 0xFFFF_FFFF;
const SESSION_IS_ROOT: u32 = 0x0001;
const SESSION_HAS_GRAPHIC_ACCESS: u32 = 0x0010;
const SESSION_HAS_TTY: u32 = 0x0020;
const SESSION_IS_REMOTE: u32 = 0x1000;

pub fn security_session() -> Value {
    let mut id: u32 = 0;
    let mut attrs: u32 = 0;
    // SAFETY: both out-params point to valid, writable u32s.
    let st = unsafe { SessionGetInfo(CALLER_SECURITY_SESSION, &mut id, &mut attrs) };
    json!({
        "status": st,
        "audit_session_id": id,
        "attrs_hex": format!("{attrs:#06x}"),
        "is_root": attrs & SESSION_IS_ROOT != 0,
        "has_graphic_access": attrs & SESSION_HAS_GRAPHIC_ACCESS != 0,
        "has_tty": attrs & SESSION_HAS_TTY != 0,
        "is_remote": attrs & SESSION_IS_REMOTE != 0,
    })
}

fn cf_dict_to_ns(d: CFRetained<CFDictionary>) -> Retained<NSDictionary<NSString, AnyObject>> {
    let raw = CFRetained::into_raw(d).as_ptr() as *mut NSDictionary<NSString, AnyObject>;
    // SAFETY: CFDictionary is toll-free bridged with NSDictionary; we transfer the +1
    // retain we own. Keys of CGSession / CGWindow dictionaries are CFStrings.
    unsafe { Retained::from_raw(raw).expect("non-null") }
}

fn cf_array_to_ns(a: CFRetained<CFArray>) -> Retained<NSArray<NSDictionary<NSString, AnyObject>>> {
    let raw = CFRetained::into_raw(a).as_ptr() as *mut NSArray<NSDictionary<NSString, AnyObject>>;
    // SAFETY: toll-free bridging; CGWindowListCopyWindowInfo returns an array of
    // CFDictionaries; we transfer our +1.
    unsafe { Retained::from_raw(raw).expect("non-null") }
}

fn get_num(d: &NSDictionary<NSString, AnyObject>, k: &str) -> Option<i64> {
    let o = d.objectForKey(&NSString::from_str(k))?;
    o.downcast_ref::<NSNumber>().map(|n| n.as_i64())
}
fn get_str(d: &NSDictionary<NSString, AnyObject>, k: &str) -> Option<String> {
    let o = d.objectForKey(&NSString::from_str(k))?;
    o.downcast_ref::<NSString>().map(|s| s.to_string())
}

/// Window-server view of the GUI session: on console, screen locked, user.
pub fn cg_session() -> Value {
    match CGSessionCopyCurrentDictionary() {
        None => json!(null),
        Some(d) => {
            let d = cf_dict_to_ns(d);
            json!({
                "on_console": get_num(&d, "kCGSSessionOnConsoleKey"),
                "secure_input_pid": get_num(&d, "kCGSSessionSecureInputPID"),
                "screen_locked_since_unix": get_num(&d, "CGSSessionScreenLockedTime"),
                "screen_is_locked": get_num(&d, "CGSSessionScreenIsLocked").unwrap_or(0),
                "login_done": get_num(&d, "kCGSessionLoginDoneKey"),
                "audit_session_id": get_num(&d, "kCGSSessionAuditIDKey"),
            })
        }
    }
}

#[derive(Debug, Clone)]
pub struct Win {
    pub number: i64,
    pub owner: String,
    pub pid: i64,
    pub layer: i64,
    pub name: Option<String>,
    pub bounds: Option<(i64, i64, i64, i64)>,
}

pub fn windows() -> Vec<Win> {
    let opts = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(arr) = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) else {
        return vec![];
    };
    let arr = cf_array_to_ns(arr);
    let mut out = Vec::new();
    for d in arr.iter() {
        let bounds = d
            .objectForKey(&NSString::from_str("kCGWindowBounds"))
            .and_then(|b| b.downcast::<NSDictionary>().ok())
            .map(|b| {
                // SAFETY: bounds dictionary has NSString keys and NSNumber values.
                let b: Retained<NSDictionary<NSString, AnyObject>> =
                    unsafe { Retained::cast_unchecked(b) };
                (
                    get_num(&b, "X").unwrap_or(0),
                    get_num(&b, "Y").unwrap_or(0),
                    get_num(&b, "Width").unwrap_or(0),
                    get_num(&b, "Height").unwrap_or(0),
                )
            });
        out.push(Win {
            number: get_num(&d, "kCGWindowNumber").unwrap_or(-1),
            owner: get_str(&d, "kCGWindowOwnerName").unwrap_or_default(),
            pid: get_num(&d, "kCGWindowOwnerPID").unwrap_or(-1),
            layer: get_num(&d, "kCGWindowLayer").unwrap_or(0),
            name: get_str(&d, "kCGWindowName"),
            bounds,
        });
    }
    out
}

pub fn win_json(w: &Win) -> Value {
    json!({"number": w.number, "owner": w.owner, "pid": w.pid, "layer": w.layer, "name": w.name, "bounds": w.bounds})
}
