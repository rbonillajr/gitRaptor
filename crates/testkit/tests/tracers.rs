//! Parsers of the kernel tracers, on captured samples (the tracers themselves need Linux or
//! root on macOS; they run in CI, see the INF-GRP-001 Dev Spec).

mod repo_intact {
    use std::path::Path;

    use gitraptor_testkit::exec_audit::{parse_eslogger, parse_strace};

    #[test]
    fn strace_execs_are_parsed_with_argv_and_failures() {
        let text = r#"100 execve("/tmp/probe", ["/tmp/probe", "--x"], ["HOME=/h"]) = 0
101 execve("/tmp/a/shim/git", ["/tmp/a/shim/git", "--no-optional-locks", "-c", "a=\"b\"", "log"], ["HOME=/h", "PATH=/t"]) = 0
102 execve("/tmp/a/trap/git", ["git", "config", "-l"], []) = -1 ENOENT (No such file or directory)
102 +++ exited with 0 +++
"#;
        let execs = parse_strace(text);
        assert_eq!(execs.len(), 3);
        assert_eq!(execs[1].program, "/tmp/a/shim/git");
        assert_eq!(
            execs[1].argv,
            [
                "/tmp/a/shim/git",
                "--no-optional-locks",
                "-c",
                "a=\"b\"",
                "log"
            ]
        );
        assert!(execs[1].ok);
        assert_eq!(execs[2].pid, 102);
        assert!(!execs[2].ok);
    }

    #[test]
    fn eslogger_tree_is_rebuilt_from_ppid() {
        let line = |pid: u32, ppid: u32, path: &str, args: &str| {
            format!(
                r#"{{"process":{{"audit_token":{{"pid":{pid}}},"ppid":{ppid}}},"event":{{"exec":{{"target":{{"executable":{{"path":"{path}"}}}},"args":[{args}]}}}}}}"#
            )
        };
        let text = [
            line(10, 1, "/tmp/probe", r#""/tmp/probe""#),
            line(11, 10, "/tmp/a/shim/git", r#""git","log""#),
            line(12, 11, "/usr/bin/git", r#""git","log""#),
            line(99, 1, "/bin/ls", r#""ls""#),
        ]
        .join("\n");
        let execs = parse_eslogger(&text, 10, Path::new("/tmp/probe"));
        let programs: Vec<_> = execs.iter().map(|e| e.program.as_str()).collect();
        assert_eq!(programs, ["/tmp/a/shim/git", "/usr/bin/git"]);
        assert_eq!(execs[0].argv, ["git", "log"]);
    }
}
