//! Contract tests of the `via` of the `s3_read_notice` log line (TD-GRP-006).

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::ReadNoticeLog;
use crate::daemon::{LOG_FILE, LogLimits, Logger};
use crate::watch::{ObserverHooks, ReadVia};

fn now_us() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    u64::try_from(since.as_micros()).unwrap()
}

/// GRP006-LOG (SEC-04): every `s3_read_notice` line says which read found the write
/// (`via=event|wake|backup`, the word of the `ReadVia` it got), besides `repo`, `age_ms` and
/// `kept`, and no value of the line is a path.
#[test]
fn grp006_log_read_notice_carries_the_via_of_each_read() {
    let tmp = tempfile::tempdir().unwrap();
    let logger = Logger::open(tmp.path(), LogLimits::default()).unwrap();
    let hooks = ReadNoticeLog(logger.clone());
    let cases = [
        ("3f2a-00ff", ReadVia::Event, "event", "3f2a-00ff"),
        ("3f2a-00ff", ReadVia::Wake, "wake", "3f2a-00ff"),
        // A repo id that is a path never reaches the line as one.
        ("/home/canary/repo", ReadVia::Backup, "backup", "invalid-id"),
        ("C:\\Users\\canary", ReadVia::Wake, "wake", "invalid-id"),
    ];
    for (repo, via, _, _) in cases {
        // A write of now: within READ_WRITE_MAX_AGE, so kept.
        hooks.git_dir_written(repo, 0, now_us(), via);
    }
    logger.flush();

    let text = std::fs::read_to_string(tmp.path().join(LOG_FILE)).unwrap();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.split(' ').nth(2) == Some("s3_read_notice"))
        .collect();
    assert_eq!(
        lines.len(),
        cases.len(),
        "one s3_read_notice line per read notice: {text}"
    );
    for (line, (_, _, want_via, want_repo)) in lines.iter().zip(cases) {
        let pairs: Vec<(&str, &str)> = line
            .split(' ')
            .skip(3)
            .map(|w| w.split_once('=').expect("key=value"))
            .collect();
        let map: HashMap<&str, &str> = pairs.iter().copied().collect();
        assert_eq!(map.len(), pairs.len(), "no key repeats: {line}");
        assert_eq!(
            map.get("via"),
            Some(&want_via),
            "s3_read_notice must say which read found the write (via={want_via}): {line}"
        );
        assert_eq!(map.get("repo"), Some(&want_repo), "{line}");
        assert!(
            map.get("age_ms").is_some_and(|v| v.parse::<u64>().is_ok()),
            "age_ms is an integer: {line}"
        );
        assert_eq!(
            map.get("kept"),
            Some(&"true"),
            "a write of now is kept: {line}"
        );
        for (key, value) in &pairs {
            assert!(
                !value.contains('/') && !value.contains('\\'),
                "{key} holds no path: {line}"
            );
        }
        assert!(!line.contains("canary"), "the repo path leaked: {line}");
    }
}
