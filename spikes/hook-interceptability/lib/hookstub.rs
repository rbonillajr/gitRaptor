// Stand-in for `raptor hook` in the cost suite (SPIKE-GRD-001, suite 06):
// a native Rust binary that reads and splits the whole stdin, as the fast
// path of ADR-GRD-002 § 4 would, and exits 0 without contacting a daemon.
// Built with `rustc -O` into the sandbox; not part of the Cargo workspace.
use std::io::Read;

fn main() {
    let mut input = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut input);
    let governed = input
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .filter(|l| l.windows(11).any(|w| w == b"refs/heads/"))
        .count();
    std::process::exit(if governed > usize::MAX - 1 { 1 } else { 0 });
}
