//! The providing power source (IOKit `IOPowerSources.h`) and the low power mode
//! (`NSProcessInfo.isLowPowerModeEnabled`, through the Objective-C runtime), one call per
//! `unsafe` block, behind safe functions. Nothing here is public outside the crate and no
//! pointer crosses its boundary. `libc` declares none of these symbols, so they are declared by
//! hand, and no Objective-C crate is used.

use std::ffi::{CStr, c_char, c_void};

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
/// `id` and `Class`: opaque object pointers of the Objective-C runtime.
type Id = *mut c_void;
/// `SEL`: an opaque selector pointer.
type Sel = *const c_void;

/// `kCFStringEncodingUTF8`.
const UTF8: u32 = 0x0800_0100;
/// `kIOPMBatteryPowerKey`, the value `IOPSGetProvidingPowerSourceType` returns on battery.
const BATTERY_POWER: &[u8] = b"Battery Power";
/// Room for any providing power source type ("AC Power", "Battery Power", "UPS Power").
const TYPE_BUF: usize = 64;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: CFTypeRef);
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> u8;
}

// Foundation is linked only so that `NSProcessInfo` is registered with the runtime.
#[allow(clippy::duplicated_attributes)]
#[link(name = "Foundation", kind = "framework")]
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn object_getClass(object: Id) -> Id;
    fn class_respondsToSelector(class: Id, selector: Sel) -> u8;
    /// Declared without a signature on purpose: it is only ever called through a pointer cast
    /// to the exact type of each message, as the runtime requires.
    fn objc_msgSend();
}

/// `objc_msgSend` for a message that takes no argument and returns an object.
type MsgSendId = unsafe extern "C" fn(Id, Sel) -> Id;
/// `objc_msgSend` for a message that takes no argument and returns a `BOOL` (read as a byte:
/// `signed char` on x86_64, `bool` on arm64, both returned in the low byte).
type MsgSendBool = unsafe extern "C" fn(Id, Sel) -> u8;

/// Releases a Core Foundation object this module owns (a `Copy`/`Create` result) on drop.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: `Owned` is only built from a non-null `Copy` result this module owns, and is
        // dropped once, so the object is released exactly once.
        unsafe { CFRelease(self.0) };
    }
}

/// Whether the providing power source is the battery; `None` when IOKit gives no answer.
///
/// A Mac without a battery reports `AC Power`, so it reads `Some(false)`.
pub(crate) fn on_battery() -> Option<bool> {
    // SAFETY: no arguments; returns a new reference (the Copy rule) or null on failure.
    let snapshot = unsafe { IOPSCopyPowerSourcesInfo() };
    if snapshot.is_null() {
        return None;
    }
    let snapshot = Owned(snapshot);
    // SAFETY: `snapshot` is the live, non-null object just copied; the result follows the Get
    // rule (not owned, valid while `snapshot` lives) or is null.
    let kind = unsafe { IOPSGetProvidingPowerSourceType(snapshot.0) };
    if kind.is_null() {
        return None;
    }
    let mut buf = [0 as c_char; TYPE_BUF];
    // SAFETY: `kind` is a valid CFString owned by `snapshot`, still alive; `buf` is writable
    // for `TYPE_BUF` bytes, the size passed, and the call writes a NUL-terminated string within
    // it or returns false.
    let ok = unsafe { CFStringGetCString(kind, buf.as_mut_ptr(), TYPE_BUF as isize, UTF8) };
    if ok == 0 {
        return None;
    }
    // `c_char` is `i8` or `u8` by target: reinterpret each byte as is.
    let bytes = buf.map(|c| c as u8);
    let kind = CStr::from_bytes_until_nul(&bytes).ok()?;
    Some(kind.to_bytes() == BATTERY_POWER)
}

/// Whether the low power mode is on; `None` when the runtime does not offer it.
pub(crate) fn low_power() -> Option<bool> {
    // SAFETY: a NUL-terminated literal; returns the registered class or null.
    let class = unsafe { objc_getClass(c"NSProcessInfo".as_ptr()) };
    if class.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated literal; registering a selector never fails.
    let process_info = unsafe { sel_registerName(c"processInfo".as_ptr()) };
    // SAFETY: a NUL-terminated literal; registering a selector never fails.
    let is_low_power = unsafe { sel_registerName(c"isLowPowerModeEnabled".as_ptr()) };
    // SAFETY: casts the untyped `objc_msgSend` to the exact signature of `+processInfo`
    // (no argument, returns an object), as the runtime requires for every call.
    let send_id: MsgSendId =
        unsafe { std::mem::transmute::<unsafe extern "C" fn(), MsgSendId>(objc_msgSend) };
    // SAFETY: `class` is the live `NSProcessInfo` class and `+processInfo` takes no argument
    // and returns the shared, never-released instance (not owned by the caller).
    let info = unsafe { send_id(class, process_info) };
    if info.is_null() {
        return None;
    }
    // SAFETY: `info` is a live object; returns its class, never null for a valid object.
    let info_class = unsafe { object_getClass(info) };
    // SAFETY: `info_class` is a live class and `is_low_power` a registered selector; only
    // reads the method table (older systems lack the property: then it answers false).
    let responds = unsafe { class_respondsToSelector(info_class, is_low_power) };
    if responds == 0 {
        return None;
    }
    // SAFETY: casts the untyped `objc_msgSend` to the exact signature of
    // `-isLowPowerModeEnabled` (no argument, returns `BOOL`).
    let send_bool: MsgSendBool =
        unsafe { std::mem::transmute::<unsafe extern "C" fn(), MsgSendBool>(objc_msgSend) };
    // SAFETY: `info` responds to `isLowPowerModeEnabled` (checked above), which takes no
    // argument and returns a `BOOL` read as a byte.
    let low = unsafe { send_bool(info, is_low_power) };
    Some(low != 0)
}
