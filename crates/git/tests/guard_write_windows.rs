//! The guardrails folder on Windows (DS-US-GRD-001, Enmienda 2026-10-08; ADR-GRD-001 § 1, M-07):
//! created with a private DACL that its files inherit, with an identity that later writes check.
//! Temporary folders only, never a real repo.
#![cfg(windows)]

use std::path::Path;

use gitraptor_git::Invoker;
use gitraptor_git::guard_write::{FOLDER, GuardWriteError, GuardWriter, NewFile};
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_winsys::acl;

const PROGRAM_FILES_GIT: &str = r"C:\Program Files\Git\cmd\git.exe";

fn files() -> Vec<NewFile<'static>> {
    vec![
        NewFile {
            path: "hooks/pre-push",
            bytes: b"stub",
            executable: true,
        },
        NewFile {
            path: "dispatch.conf",
            bytes: b"template\t2\n",
            executable: false,
        },
    ]
}

fn with_writer(test: impl FnOnce(&GuardWriter<'_>, &Path)) {
    let config = ResolveConfig {
        configured_path: Some(PROGRAM_FILES_GIT.into()),
        path_env: None,
        known_locations: vec![],
        shim_paths: vec![],
        toolchain_gits: vec![],
    };
    let invoker = Invoker::default();
    let Resolution::Found { git, .. } = resolve::resolve(&config, &invoker) else {
        panic!("no system Git to build the writer with");
    };
    let tmp = tempfile::tempdir().unwrap();
    let common = tmp.path().join("path with spaces").join(".git");
    std::fs::create_dir_all(&common).unwrap();
    test(&GuardWriter::new(&git, &invoker), &common);
}

#[test]
fn the_folder_and_its_files_are_private_to_the_user() {
    with_writer(|writer, common| {
        writer.write_folder(common, &files()).unwrap();
        let folder = common.join(FOLDER);
        acl::verify_private_dir(&folder).unwrap();
        acl::verify_private_dir(&folder.join("hooks")).unwrap();
        // What a file inside inherited: Users and Everyone cannot write the dispatcher.
        let out = std::process::Command::new("icacls")
            .arg(folder.join("hooks").join("pre-push"))
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&out.stdout).to_lowercase();
        assert!(!listed.contains("everyone"), "{listed}");
        assert!(!listed.contains("builtin\\users"), "{listed}");
        assert!(!listed.contains("authenticated users"), "{listed}");
    });
}

#[test]
fn a_replaced_folder_is_not_written_into() {
    with_writer(|writer, common| {
        let id = writer.write_folder(common, &files()).unwrap();
        assert_eq!(writer.folder_id(common).unwrap(), Some(id));
        writer.replace_files(common, id, &files()).unwrap();
        // Another folder under the same name: what the journal recorded is not it.
        let folder = common.join(FOLDER);
        let moved = common.join("moved");
        std::fs::rename(&folder, &moved).unwrap();
        std::fs::create_dir(&folder).unwrap();
        assert!(matches!(
            writer.replace_files(common, id, &files()),
            Err(GuardWriteError::Changed(_))
        ));
        let listed: Vec<&str> = files().iter().map(|f| f.path).collect();
        assert!(matches!(
            writer.remove_folder(common, &listed, id),
            Err(GuardWriteError::Changed(_))
        ));
    });
}
