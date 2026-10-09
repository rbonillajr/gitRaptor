//! One `raptor-mcp` process driven over stdio: the handshake, the case's messages, a sentinel,
//! and every read bounded by a deadline (no wait without a limit, no fixed sleep).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use gitraptor_testkit::mcp_corpus::case::Send as Step;
use gitraptor_testkit::mcp_corpus::{Answer, Observation};
use serde_json::{Map, Value, json};

use super::machine::Env;

/// Longest wait for one more line from the server.
const READ_DEADLINE: Duration = Duration::from_secs(15);
/// Longest a whole case may take.
const CASE_DEADLINE: Duration = Duration::from_secs(60);
const SENTINEL_ID: u64 = 9999;
const FIRST_CALL_ID: u64 = 10;
const FAKE_AGENT_ARGV: &str = "RAPTOR_FAKE_AGENT_ARGV";
/// The libtest line the simulated agent leaves in front of the server's first answer.
const AGENT_PREFIX: &str = "test fake_agent_entry ... ";

/// How the server is started.
pub struct Launch {
    pub server: PathBuf,
    pub cwd: PathBuf,
    pub env: Env,
    /// The simulated agent that launches the server, if any.
    pub agent: Option<PathBuf>,
}

/// What the session showed. The repo and trap checks belong to the caller.
pub struct Seen {
    pub observation: Observation,
}

impl Launch {
    fn command(&self) -> Command {
        let mut cmd = match &self.agent {
            None => Command::new(&self.server),
            Some(agent) => {
                let mut cmd = Command::new(agent);
                cmd.args([
                    "fake_agent_entry",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ]);
                cmd
            }
        };
        cmd.env_clear().envs(self.env.iter().cloned());
        if self.agent.is_some() {
            let argv = serde_json::to_string(&[self.server.to_string_lossy()])
                .unwrap_or_else(|e| panic!("cannot encode the agent argv: {e}"));
            cmd.env(FAKE_AGENT_ARGV, argv);
        }
        cmd.current_dir(&self.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}

/// The server's line, or `None` for a line that belongs to the simulated agent's test harness.
fn agent_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix(AGENT_PREFIX).unwrap_or(line);
    let harness = rest.is_empty()
        || rest == "ok"
        || rest.starts_with("test result:")
        || (rest.starts_with("running ") && rest.ends_with(" tests"))
        || rest == "running 1 test";
    (!harness).then(|| rest.to_owned())
}

enum Heard {
    Line,
    Closed,
    TimedOut,
}

struct Io {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    started: Instant,
    under_agent: bool,
    stdout: Vec<String>,
    inbox: Vec<Value>,
}

impl Io {
    fn write(&mut self, text: &str) -> bool {
        let Some(stdin) = self.stdin.as_mut() else {
            return false;
        };
        let mut line = String::with_capacity(text.len() + 1);
        line.push_str(text);
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .is_ok()
    }

    fn send(&mut self, message: &Value) -> bool {
        self.write(&message.to_string())
    }

    /// One more stdout line, within the read deadline and what is left of the case's.
    fn hear(&mut self) -> Heard {
        let left = CASE_DEADLINE.saturating_sub(self.started.elapsed());
        if left.is_zero() {
            return Heard::TimedOut;
        }
        match self.lines.recv_timeout(READ_DEADLINE.min(left)) {
            Ok(raw) => {
                let line = if self.under_agent {
                    agent_line(&raw)
                } else {
                    Some(raw)
                };
                if let Some(line) = line {
                    if let Ok(message) = serde_json::from_str::<Value>(&line) {
                        self.inbox.push(message);
                    }
                    self.stdout.push(line);
                }
                Heard::Line
            }
            Err(RecvTimeoutError::Timeout) => Heard::TimedOut,
            Err(RecvTimeoutError::Disconnected) => Heard::Closed,
        }
    }

    /// Hears lines until `done` holds. `false` when the server closed stdout first; a deadline
    /// kills the server and fails the harness.
    fn wait(&mut self, what: &str, mut done: impl FnMut(&[Value]) -> bool) -> bool {
        loop {
            if done(&self.inbox) {
                return true;
            }
            match self.hear() {
                Heard::Line => {}
                Heard::Closed => return false,
                Heard::TimedOut => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    panic!("timeout waiting for {what}");
                }
            }
        }
    }
}

fn has_id(inbox: &[Value], from: usize, id: u64) -> bool {
    inbox
        .get(from..)
        .is_some_and(|tail| tail.iter().any(|m| m["id"] == id))
}

/// Starts the server, runs the whole script and closes it as the client does at the end.
///
/// # Panics
/// On a harness failure: the process cannot start, or a read or the case runs out of time.
pub fn run(launch: &Launch, steps: &[Step]) -> Seen {
    let mut child = launch
        .command()
        .spawn()
        .unwrap_or_else(|e| panic!("cannot start raptor-mcp: {e}"));
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, lines) = channel::<String>();
    let reader = stdout.map(|out| {
        std::thread::spawn(move || {
            let mut out = BufReader::new(out);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match out.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&buf);
                        let line = text.trim_end_matches(['\n', '\r']).to_owned();
                        if tx.send(line).is_err() {
                            return;
                        }
                    }
                }
            }
        })
    });
    let err_reader = stderr.map(|mut err| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    });
    let mut io = Io {
        child,
        stdin,
        lines,
        started: Instant::now(),
        under_agent: launch.agent.is_some(),
        stdout: Vec::new(),
        inbox: Vec::new(),
    };

    let (tools, answers, sentinel) = converse(&mut io, steps);

    // Close stdin as the client does, then wait for the server to end: stdout closing is the
    // signal, bounded by the deadlines.
    drop(io.stdin.take());
    loop {
        match io.hear() {
            Heard::Line => {}
            Heard::Closed => break,
            Heard::TimedOut => {
                let _ = io.child.kill();
                let _ = io.child.wait();
                panic!("timeout waiting for raptor-mcp to exit");
            }
        }
    }
    let _ = io.child.wait();
    if let Some(reader) = reader {
        let _ = reader.join();
    }
    let stderr = err_reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();

    Seen {
        observation: Observation {
            tools,
            answers,
            stdout: io.stdout,
            stderr,
            sentinel_answered: sentinel,
            ..Observation::default()
        },
    }
}

/// Handshake, `steps` and the sentinel: the tools the server lists, the answers in arrival
/// order, and whether the sentinel was answered.
fn converse(io: &mut Io, steps: &[Step]) -> (Vec<Value>, Vec<Answer>, bool) {
    let mut tools = Vec::new();
    let mut tool_of: HashMap<u64, String> = HashMap::new();

    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "claude-code", "version": "2.1.284"}
        }
    });
    if !io.send(&init) || !io.wait("the initialize answer", |m| has_id(m, 0, 1)) {
        return (tools, Vec::new(), false);
    }
    let listing = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    if !io.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        || !io.send(&listing)
        || !io.wait("the tools/list answer", |m| has_id(m, 0, 2))
    {
        return (tools, Vec::new(), false);
    }
    if let Some(listed) = io
        .inbox
        .iter()
        .find(|m| m["id"] == 2)
        .and_then(|m| m["result"]["tools"].as_array())
    {
        tools = listed.clone();
    }
    let mark = io.inbox.len();

    let mut next = FIRST_CALL_ID;
    let mut alive = true;
    'steps: for step in steps {
        match step {
            Step::Raw(line) => {
                alive = io.write(line);
            }
            Step::Message(message) => {
                let mut body: Map<String, Value> = message.clone();
                body.insert("jsonrpc".into(), json!("2.0"));
                body.insert("id".into(), json!(next));
                next += 1;
                let want = io.inbox.len() + 1;
                alive = io.send(&Value::Object(body))
                    && io.wait("a message answer", |m| m.len() >= want);
            }
            Step::Call {
                tool,
                arguments,
                repeat,
            } => {
                // Serial on purpose: the server answers a call that finds the engine connection
                // busy at once (`engine-unavailable` / `time-limit`), so a pipelined burst never
                // reaches the rate limit; the budget is spent by calls that each wait their answer.
                for _ in 0..*repeat {
                    let id = next;
                    next += 1;
                    let mut params = json!({"name": tool});
                    if let Some(arguments) = arguments {
                        params["arguments"] = arguments.clone();
                    }
                    tool_of.insert(id, tool.clone());
                    let call = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params});
                    alive = io.send(&call) && io.wait("a tool answer", |m| has_id(m, mark, id));
                    if !alive {
                        break 'steps;
                    }
                }
            }
        }
        if !alive {
            break;
        }
    }

    let mut sentinel = false;
    if alive {
        let ping = json!({"jsonrpc": "2.0", "id": SENTINEL_ID, "method": "ping"});
        sentinel =
            io.send(&ping) && io.wait("the sentinel answer", |m| has_id(m, mark, SENTINEL_ID));
    }
    let answers = io.inbox[mark..]
        .iter()
        .filter(|m| m["id"] != SENTINEL_ID)
        .map(|message| Answer {
            tool: message["id"]
                .as_u64()
                .and_then(|id| tool_of.get(&id).cloned()),
            message: message.clone(),
        })
        .collect();
    (tools, answers, sentinel)
}
