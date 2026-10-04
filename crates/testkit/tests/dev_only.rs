//! The testkit is test support: no package of the workspace may depend on it except as a
//! dev-dependency (ADR-GRP-002, nota de integración INF-GRP-001). Checked on `cargo metadata`,
//! which resolves the workspace exactly as Cargo builds it.

mod repo_intact {
    #[test]
    fn testkit_is_only_a_dev_dependency() {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let out = std::process::Command::new(cargo)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo metadata");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let mut offenders = Vec::new();
        let mut users = 0;
        for pkg in meta["packages"].as_array().unwrap() {
            for dep in pkg["dependencies"].as_array().unwrap() {
                if dep["name"] == "gitraptor-testkit" {
                    if dep["kind"] == "dev" {
                        users += 1;
                    } else {
                        offenders.push(format!("{} ({})", pkg["name"], dep["kind"]));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "testkit as a regular dependency: {offenders:?}"
        );
        assert!(users > 0, "no crate uses the testkit");
    }
}
