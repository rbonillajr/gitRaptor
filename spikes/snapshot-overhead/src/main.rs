//! SPIKE-TMC-001 prototype. Isolated: not part of the GitRaptor workspace, never touches
//! `crates/` or `apps/`, and refuses to run inside an existing Git repository.

mod bench;
mod repogen;
mod snap;
mod util;

use util::{Res, err};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() -> Res<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(arg(&args, "--root").ok_or("--root <dir> is required")?);
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    // NFR-01 guard: the bench only works on repos it generated in a scratch directory.
    let mut c = util::git();
    c.current_dir(&root).args(["rev-parse", "--show-toplevel"]);
    if util::run(c).is_ok() {
        return err(format!(
            "{} is inside a Git repository; use a scratch directory",
            root.display()
        ));
    }
    let variants = arg(&args, "--variants")
        .unwrap_or_else(|| "cli,fi,fi-hint,gix-hint".into())
        .split(',')
        .map(snap::Variant::parse)
        .collect::<Res<Vec<_>>>()?;
    let opts = bench::Opts {
        profile: arg(&args, "--profile").unwrap_or_else(|| "M".into()),
        root,
        variants,
        iters: arg(&args, "--iters").map_or(Ok(100), |s| s.parse())?,
        multi: !args.iter().any(|a| a == "--no-multi"),
        week: arg(&args, "--week").map_or(Ok(2400), |s| s.parse())?,
        big: !args.iter().any(|a| a == "--no-big"),
        seed_exp: !args.iter().any(|a| a == "--no-seed"),
    };
    match args.get(1).map(String::as_str) {
        Some("bench") => bench::run_all(&opts),
        _ => err(
            "usage: snapshot-overhead bench --root <dir> [--profile S|P50|M|L] [--variants cli,fi,fi-hint,gix-hint] [--iters N] [--week N] [--no-multi] [--no-big] [--no-seed]",
        ),
    }
}
