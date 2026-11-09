//! D3-STOPCMD (DS-US-DSK-003 T004 step 2, M-1 of ADR-GRP-017 § 5): `stop_command` builds the command
//! that stops the daemon at a path, safe to paste in a POSIX shell: every argument in single quotes,
//! a quote written `'\''`, and nothing at all when a character meets `must_escape`.

use gitraptor_api::display::must_escape;
use gitraptor_desktop::engine::wire::stop_command;

#[test]
fn stop_command_quotes_every_argument_and_refuses_sec12() {
    assert_eq!(
        stop_command("/opt/gitraptor/raptor").as_deref(),
        Some("'/opt/gitraptor/raptor' 'daemon' 'stop'"),
        "D3-STOPCMD: every argument is single-quoted"
    );
    assert_eq!(
        stop_command("/Applications/Git Raptor.app/raptor").as_deref(),
        Some("'/Applications/Git Raptor.app/raptor' 'daemon' 'stop'"),
        "D3-STOPCMD: a space stays inside the quotes"
    );
    assert_eq!(
        stop_command("/o'brien/raptor").as_deref(),
        Some("'/o'\\''brien/raptor' 'daemon' 'stop'"),
        "D3-STOPCMD: a single quote is written '\\''"
    );
    assert_eq!(
        stop_command("/w/$HOME/`id`;x").as_deref(),
        Some("'/w/$HOME/`id`;x' 'daemon' 'stop'"),
        "D3-STOPCMD: shell metacharacters are inert inside single quotes"
    );
    for c in ['\u{1b}', '\u{7}', '\n', '\u{202e}', '\u{200b}', '\u{85}'] {
        assert!(must_escape(c), "the vector {c:?} must meet must_escape");
        assert_eq!(
            stop_command(&format!("/opt/{c}/raptor")),
            None,
            "D3-STOPCMD: a path with {c:?} (SEC-12) must give no command (M-1)"
        );
    }
}
