//! What the current process consumes (US-GRP-017): CPU time, working set and open handles.
//! Windows only; the other OSes measure themselves in `gitraptor-core`.

/// One reading of the current process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessUsage {
    /// User plus kernel CPU time, in nanoseconds.
    pub cpu_ns: Option<u64>,
    /// Working set, in bytes: the resident memory of Windows.
    pub resident_bytes: Option<u64>,
    /// Open handles: the descriptors of Windows.
    pub handles: Option<u64>,
}

/// Reads the current process. A value Windows does not give is `None`.
pub fn current() -> ProcessUsage {
    ProcessUsage {
        cpu_ns: crate::ffi_usage::cpu_time_100ns().map(|t| t.saturating_mul(100)),
        resident_bytes: crate::ffi_usage::working_set_bytes(),
        handles: crate::ffi_usage::handle_count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_process_has_memory_time_and_handles() {
        let u = current();
        assert!(u.resident_bytes.unwrap() > 0);
        assert!(u.handles.unwrap() > 0);
        let before = u.cpu_ns.unwrap();
        let mut x = 0u64;
        for i in 0..20_000_000u64 {
            x = x.wrapping_add(i * i);
        }
        std::hint::black_box(x);
        assert!(current().cpu_ns.unwrap() >= before);
    }
}
