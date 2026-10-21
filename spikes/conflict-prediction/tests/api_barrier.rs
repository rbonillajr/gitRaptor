//! E5 — API barrier: the A2 module never reaches the high-level gix merge API, which builds its
//! resource caches from the observed repository (merge drivers, filters, `.gitattributes`; F4).

const FORBIDDEN: &[&str] = &[
    "merge_trees",
    "merge_commits",
    "merge_resource_cache",
    "tree_merge_options",
];

fn code_of(path: &str) -> String {
    let src = std::fs::read_to_string(path).expect("read source");
    // Comments may name the forbidden calls to explain the barrier; only code counts.
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a2_never_calls_the_high_level_merge_api() {
    let code = code_of(concat!(env!("CARGO_MANIFEST_DIR"), "/src/a2.rs"));
    for f in FORBIDDEN {
        assert!(!code.contains(f), "src/a2.rs calls `{f}`");
    }
}

#[test]
fn the_barrier_catches_the_control() {
    // A1 is the control that uses the forbidden API: the check must see it there.
    let code = code_of(concat!(env!("CARGO_MANIFEST_DIR"), "/src/a1.rs"));
    assert!(FORBIDDEN.iter().any(|f| code.contains(f)));
}
