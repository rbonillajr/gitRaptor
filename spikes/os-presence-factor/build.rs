// Optional: embed an Info.plist in the Mach-O (`-sectcreate __TEXT __info_plist`) so
// LocalAuthentication can read a display name without an .app bundle (SPIKE-GRD-002 E9, F7).
fn main() {
    println!("cargo:rerun-if-env-changed=PROBE_INFO_PLIST");
    if let Ok(p) = std::env::var("PROBE_INFO_PLIST") {
        println!("cargo:rerun-if-changed={p}");
        println!("cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{p}");
    }
}
