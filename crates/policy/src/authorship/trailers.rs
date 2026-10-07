//! `Co-Authored-By` trailers of a commit message, by the rules of `git interpret-trailers` for
//! the **last paragraph** (DS-US-GRD-018 § 3): `key: value` lines, key without case,
//! continuation lines that start with whitespace, the title paragraph never a trailer block, and
//! comment lines dropped as `commit.cleanup` drops them. `commit-msg` gets the message before
//! the cleanup (D7), so the mode and `core.commentChar` are inputs.

/// Longest message read (D7).
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Most co-authors read.
pub const MAX_COAUTHORS: usize = 32;

/// `--cleanup` / `commit.cleanup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cleanup {
    /// `strip` (and `default` with an editor): comment lines go.
    #[default]
    Strip,
    /// Everything below the scissors line goes, then like `strip`.
    Scissors,
    /// Comment lines stay.
    Whitespace,
    /// Comment lines stay.
    Verbatim,
}

impl Cleanup {
    /// The mode of a `commit.cleanup` value; an unknown value is Git's `default`.
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "scissors" => Self::Scissors,
            "whitespace" => Self::Whitespace,
            "verbatim" => Self::Verbatim,
            _ => Self::Strip,
        }
    }
}

/// How the message is cleaned before its trailers are read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageOptions {
    pub cleanup: Cleanup,
    /// `core.commentChar` (`#` by default). `auto` is resolved by the caller.
    pub comment: String,
}

impl Default for MessageOptions {
    fn default() -> Self {
        Self {
            cleanup: Cleanup::Strip,
            comment: "#".into(),
        }
    }
}

/// One `Co-Authored-By` value. A value that is not `Name <email>` keeps an empty email, so it
/// counts as a co-author without a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoAuthor {
    pub name: String,
    pub email: String,
}

/// Prefixes Git itself writes: with one of them, a block of 25 % trailers is a trailer block.
const GIT_GENERATED: &[&str] = &["Signed-off-by: ", "(cherry picked from commit "];

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// The key of a `key: value` line, if it is one.
fn key_of(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let (key, _) = line.split_once(':')?;
    let key = key.trim_end();
    (!key.is_empty() && !key.contains(char::is_whitespace)).then_some(key)
}

/// The lines left once the cleanup mode is applied.
fn cleaned<'a>(message: &'a str, options: &MessageOptions) -> Vec<&'a str> {
    let comment = if options.comment.is_empty() {
        "#"
    } else {
        options.comment.as_str()
    };
    let scissors = format!("{comment} ------------------------ >8 ------------------------");
    let mut out = Vec::new();
    for line in message.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match options.cleanup {
            Cleanup::Scissors if line == scissors => break,
            Cleanup::Strip | Cleanup::Scissors if line.starts_with(comment) => continue,
            _ => out.push(line),
        }
    }
    out
}

/// Every `Co-Authored-By` of the last paragraph, when it is a trailer block.
pub fn coauthors(message: &str, options: &MessageOptions) -> Vec<CoAuthor> {
    let lines = cleaned(message, options);
    let end = lines
        .iter()
        .rposition(|l| !is_blank(l))
        .map_or(0, |i| i + 1);
    let lines = &lines[..end];
    // The first paragraph is the title and cannot be trailers.
    let title_end = lines
        .iter()
        .position(|l| is_blank(l))
        .unwrap_or(lines.len());
    let start = lines
        .iter()
        .rposition(|l| is_blank(l))
        .map_or(lines.len(), |i| i + 1);
    if start <= title_end || start >= lines.len() {
        return Vec::new();
    }
    let block = &lines[start..];

    // Unfold continuation lines into their trailer.
    let mut entries: Vec<(Option<&str>, String)> = Vec::new();
    for line in block {
        if line.starts_with(char::is_whitespace)
            && let Some((Some(_), value)) = entries.last_mut()
        {
            value.push(' ');
            value.push_str(line.trim());
            continue;
        }
        match key_of(line) {
            Some(key) => {
                let value = line.split_once(':').map_or("", |(_, v)| v).trim();
                entries.push((Some(key), value.to_owned()));
            }
            None => entries.push((None, String::new())),
        }
    }
    let trailers = entries.iter().filter(|(k, _)| k.is_some()).count();
    let others = entries.len() - trailers;
    let generated = block
        .iter()
        .any(|l| GIT_GENERATED.iter().any(|p| l.starts_with(p)));
    let is_block = trailers > 0 && (others == 0 || (generated && trailers * 3 >= others));
    if !is_block {
        return Vec::new();
    }
    entries
        .into_iter()
        .filter_map(|(key, value)| {
            key.filter(|k| k.eq_ignore_ascii_case("co-authored-by"))
                .map(|_| split_identity(&value))
        })
        .take(MAX_COAUTHORS)
        .collect()
}

fn split_identity(value: &str) -> CoAuthor {
    match (value.find('<'), value.rfind('>')) {
        (Some(open), Some(close)) if open < close && close == value.len() - 1 => CoAuthor {
            name: value[..open].trim().to_owned(),
            email: value[open + 1..close].trim().to_owned(),
        },
        _ => CoAuthor {
            name: value.trim().to_owned(),
            email: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = "Co-Authored-By: Claude <noreply@anthropic.com>";

    fn names(message: &str) -> Vec<String> {
        coauthors(message, &MessageOptions::default())
            .into_iter()
            .map(|c| format!("{} <{}>", c.name, c.email))
            .collect()
    }

    /// The corpus of § 3: (message, expected `Co-Authored-By` values).
    fn corpus() -> Vec<(String, Vec<&'static str>)> {
        vec![
            (
                format!("feat: x\n\nbody\n\n{CLAUDE}\n"),
                vec!["Claude <noreply@anthropic.com>"],
            ),
            // In the middle of the body: not the last paragraph.
            (format!("feat: x\n\n{CLAUDE}\n\nmore body\n"), vec![]),
            // Comment lines below the trailers (what `commit-msg` sees).
            (
                format!("feat: x\n\n{CLAUDE}\n# Please enter the commit message\n# x\n"),
                vec!["Claude <noreply@anthropic.com>"],
            ),
            // Mixed with Signed-off-by.
            (
                format!("feat: x\n\nSigned-off-by: Ana <ana@x.com>\n{CLAUDE}\n"),
                vec!["Claude <noreply@anthropic.com>"],
            ),
            // CRLF.
            (
                format!("feat: x\r\n\r\n{CLAUDE}\r\n"),
                vec!["Claude <noreply@anthropic.com>"],
            ),
            // No blank line before: the title paragraph.
            (format!("feat: x\n{CLAUDE}\n"), vec![]),
            // Other case of the key.
            (
                "feat: x\n\nco-authored-by: Claude Opus <noreply@anthropic.com>\n".into(),
                vec!["Claude Opus <noreply@anthropic.com>"],
            ),
            // A prose line in the block without a Git-generated trailer: no block.
            (format!("feat: x\n\nsome words here\n{CLAUDE}\n"), vec![]),
            // With Signed-off-by, 25 % trailers suffice.
            (
                format!("feat: x\n\nwords\nSigned-off-by: Ana <ana@x.com>\n{CLAUDE}\n"),
                vec!["Claude <noreply@anthropic.com>"],
            ),
            // Continuation line.
            (
                "feat: x\n\nCo-Authored-By: Claude\n <noreply@anthropic.com>\n".into(),
                vec!["Claude <noreply@anthropic.com>"],
            ),
            // Two co-authors.
            (
                format!("feat: x\n\n{CLAUDE}\nCo-Authored-By: Ana <ana@x.com>\n"),
                vec!["Claude <noreply@anthropic.com>", "Ana <ana@x.com>"],
            ),
        ]
    }

    #[test]
    fn corpus_expectations() {
        for (message, expected) in corpus() {
            assert_eq!(names(&message), expected, "{message:?}");
        }
    }

    /// § 3: the parser agrees with `git interpret-trailers --parse` on the corpus.
    #[test]
    fn conforms_to_git_interpret_trailers() {
        let home = tempfile::tempdir().unwrap();
        for (message, _) in corpus() {
            let mut child = std::process::Command::new("git")
                .args(["interpret-trailers", "--parse"])
                .env("HOME", home.path())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", home.path().join("none"))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("git");
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(message.as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            let text = String::from_utf8(out.stdout).unwrap();
            let theirs: Vec<String> = text
                .lines()
                .filter_map(|l| l.split_once(": "))
                .filter(|(k, _)| k.eq_ignore_ascii_case("co-authored-by"))
                .map(|(_, v)| v.trim().to_owned())
                .collect();
            assert_eq!(names(&message), theirs, "{message:?}");
        }
    }

    #[test]
    fn comment_char_and_cleanup_modes() {
        let msg = format!("feat: x\n\n{CLAUDE}\n; a comment\n");
        let semicolon = |cleanup| MessageOptions {
            cleanup,
            comment: ";".into(),
        };
        // `;` comments go with strip: the trailer stays the last paragraph.
        assert_eq!(coauthors(&msg, &semicolon(Cleanup::Strip)).len(), 1);
        // Verbatim keeps the comment line: a prose line in the block, no block.
        assert_eq!(coauthors(&msg, &semicolon(Cleanup::Verbatim)).len(), 0);
        // With `#`, `;` is not a comment.
        assert_eq!(coauthors(&msg, &MessageOptions::default()).len(), 0);

        let scissors = format!(
            "feat: x\n\n{CLAUDE}\n# ------------------------ >8 ------------------------\ndiff --git a b\n\nCo-Authored-By: Ana <ana@x.com>\n"
        );
        let opts = MessageOptions {
            cleanup: Cleanup::Scissors,
            ..MessageOptions::default()
        };
        assert_eq!(
            coauthors(&scissors, &opts),
            vec![CoAuthor {
                name: "Claude".into(),
                email: "noreply@anthropic.com".into()
            }]
        );
        assert_eq!(Cleanup::from_config("Scissors"), Cleanup::Scissors);
        assert_eq!(Cleanup::from_config("default"), Cleanup::Strip);
    }

    #[test]
    fn malformed_values_and_limits() {
        let msg = "feat: x\n\nCo-Authored-By: just a name\n";
        assert_eq!(
            coauthors(msg, &MessageOptions::default()),
            vec![CoAuthor {
                name: "just a name".into(),
                email: String::new()
            }]
        );
        let many: String = (0..40)
            .map(|i| format!("Co-Authored-By: A{i} <a{i}@x.com>\n"))
            .collect();
        let msg = format!("feat: x\n\n{many}");
        assert_eq!(
            coauthors(&msg, &MessageOptions::default()).len(),
            MAX_COAUTHORS
        );
        assert!(coauthors("", &MessageOptions::default()).is_empty());
    }
}
