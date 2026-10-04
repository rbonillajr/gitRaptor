//! Prototype of the ADR-GRP-010 observer: one shared `notify` watcher, per-worktree
//! fixed debounce window, incremental recompute with an in-memory stat cache,
//! SQLite persistence before publish, publication over a local socket,
//! backup poll and full reconciliation. Read-only towards the observed repo.

use crate::util::{FileSig, hash_file, now_ns, sig};
use anyhow::{Result, anyhow};
use bstr::{BString, ByteSlice};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone, Debug, serde::Serialize)]
pub struct EngineCfg {
    pub window_ms: u64,
    /// Sliding (trailing) debounce instead of the ADR fixed window.
    pub sliding: bool,
    pub fullfsync: bool,
    pub poll_backup_ms: Option<u64>,
    pub base_branch: String,
    pub profile_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct WtDesc {
    pub name: String,
    pub root: PathBuf,
    pub gitdir: PathBuf,
}

/// One published event, as seen by the subscriber.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct Pub {
    pub seq: u64,
    pub wt: String,
    pub kind: String,
    pub events_in_batch: usize,
    pub t_recv: u64,
    pub t_flush: u64,
    pub t_computed: u64,
    pub t_persisted: u64,
    pub t_published: u64,
    #[serde(default)]
    pub t_client_recv: u64,
    pub head_ref: String,
    pub head_oid: String,
    pub index_sig: String,
    pub dirty: usize,
    pub staged: usize,
    pub ahead: i64,
    pub behind: i64,
    pub ops: Vec<String>,
    pub changed_total: usize,
    pub changed: Vec<(String, char, String)>,
    pub full_recompute: bool,
}

enum WMsg {
    Paths { t_recv: u64, paths: Vec<PathBuf> },
    Reconcile { kind: &'static str },
    Stop,
}

struct Route {
    name: String,
    root: PathBuf,
    gitdir: PathBuf,
    tx: Sender<WMsg>,
    ignore: Gitignore,
    branch: Arc<Mutex<String>>,
}

#[derive(Default, serde::Serialize, Clone, Debug)]
pub struct Counters {
    pub notify_events: u64,
    pub paths: u64,
    pub filtered: u64,
    pub rescans: u64,
    pub errors: u64,
    pub poll_cycles: u64,
    pub poll_triggered: u64,
    pub watch_restarts: u64,
}

#[derive(Default)]
struct AtomicCounters {
    notify_events: AtomicU64,
    paths: AtomicU64,
    filtered: AtomicU64,
    rescans: AtomicU64,
    errors: AtomicU64,
    poll_cycles: AtomicU64,
    poll_triggered: AtomicU64,
    watch_restarts: AtomicU64,
}

pub struct Shared {
    cfg: EngineCfg,
    common: PathBuf,
    routes: RwLock<Vec<Route>>,
    mgr_tx: Mutex<Sender<String>>,
    pub drop_events: AtomicBool,
    counters: AtomicCounters,
    store: Mutex<rusqlite::Connection>,
    publisher: Mutex<Box<dyn Write + Send>>,
    seq: AtomicU64,
    watcher: Mutex<Option<RecommendedWatcher>>,
    fingerprints: Mutex<HashMap<String, String>>,
    /// Gap probe: every routed path under this prefix is recorded.
    pub probe: Mutex<Option<(PathBuf, HashSet<PathBuf>, u64)>>,
    workers: Mutex<HashMap<String, JoinHandle<()>>>,
    stopping: AtomicBool,
}

pub struct Engine {
    pub shared: Arc<Shared>,
    pub rx: Receiver<Pub>,
    threads: Vec<JoinHandle<()>>,
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Discover worktrees of `main_root` and validate the `gitdir` link in both directions (SEC-11).
pub fn discover(main_root: &Path) -> Vec<WtDesc> {
    let main_root = canon(main_root);
    let common = main_root.join(".git");
    let mut out = vec![WtDesc {
        name: "main".into(),
        root: main_root.clone(),
        gitdir: common.clone(),
    }];
    if let Ok(rd) = std::fs::read_dir(common.join("worktrees")) {
        let mut names: Vec<_> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for n in names {
            if let Some(d) = validate_linked(&common, &n) {
                out.push(d);
            }
        }
    }
    out
}

fn validate_linked(common: &Path, name: &str) -> Option<WtDesc> {
    let gitdir = common.join("worktrees").join(name);
    let link = std::fs::read_to_string(gitdir.join("gitdir")).ok()?;
    let dotgit = PathBuf::from(link.trim());
    let root = canon(dotgit.parent()?);
    let back = std::fs::read_to_string(root.join(".git")).ok()?;
    let back = canon(Path::new(back.trim().strip_prefix("gitdir:")?.trim()));
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| canon(&h));
    let repo_root = common.parent()?;
    let forbidden =
        root == Path::new("/") || Some(&root) == home.as_ref() || repo_root.starts_with(&root);
    if back != canon(&gitdir) || forbidden {
        return None;
    }
    // The index must exist: `git worktree add` writes it after checkout.
    if !gitdir.join("index").exists() {
        return None;
    }
    Some(WtDesc {
        name: name.to_string(),
        root,
        gitdir: canon(&gitdir),
    })
}

fn build_ignore(root: &Path) -> Gitignore {
    let mut b = GitignoreBuilder::new(root);
    let _ = b.add(root.join(".gitignore"));
    b.build().unwrap_or_else(|_| Gitignore::empty())
}

impl Engine {
    pub fn start(main_root: &Path, cfg: EngineCfg) -> Result<Engine> {
        let main_root = canon(main_root);
        let common = main_root.join(".git");
        std::fs::create_dir_all(&cfg.profile_dir)?;
        let db = rusqlite::Connection::open(cfg.profile_dir.join("engine.sqlite"))?;
        db.execute_batch(&format!(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA fullfsync={};
             CREATE TABLE IF NOT EXISTS wt_state(name TEXT PRIMARY KEY, state TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY, wt TEXT, batch INTEGER, path TEXT, status TEXT, t INTEGER);",
            if cfg.fullfsync { "ON" } else { "OFF" }
        ))?;
        let (pub_w, sub_r) = socket_pair()?;
        let (mgr_tx, mgr_rx) = channel::<String>();
        let shared = Arc::new(Shared {
            cfg: cfg.clone(),
            common: common.clone(),
            routes: RwLock::new(Vec::new()),
            mgr_tx: Mutex::new(mgr_tx),
            drop_events: AtomicBool::new(false),
            counters: AtomicCounters::default(),
            store: Mutex::new(db),
            publisher: Mutex::new(pub_w),
            seq: AtomicU64::new(0),
            watcher: Mutex::new(None),
            fingerprints: Mutex::new(HashMap::new()),
            probe: Mutex::new(None),
            workers: Mutex::new(HashMap::new()),
            stopping: AtomicBool::new(false),
        });
        let mut threads = Vec::new();

        // Subscriber (stands in for a client of the local channel).
        let (pub_tx, pub_rx) = channel::<Pub>();
        threads.push(std::thread::spawn(move || {
            let r = BufReader::new(sub_r);
            for line in r.lines() {
                let Ok(line) = line else { break };
                let t = now_ns();
                if let Ok(mut p) = serde_json::from_str::<Pub>(&line) {
                    p.t_client_recv = t;
                    if pub_tx.send(p).is_err() {
                        break;
                    }
                }
            }
        }));

        // Watcher: one per process, recursive on each worktree root (the main root covers `.git`).
        let sh = shared.clone();
        let watcher =
            notify::recommended_watcher(move |res: notify::Result<Event>| sh.on_event(res))?;
        *shared.watcher.lock().unwrap() = Some(watcher);

        for d in discover(&main_root) {
            shared.register(d)?;
        }

        // Membership manager: worktrees that appear or disappear.
        let sh = shared.clone();
        threads.push(std::thread::spawn(move || {
            while let Ok(name) = mgr_rx.recv() {
                if sh.stopping.load(Relaxed) {
                    break;
                }
                sh.check_membership(&name);
            }
        }));

        // Backup poll.
        if let Some(every) = cfg.poll_backup_ms {
            let sh = shared.clone();
            threads.push(std::thread::spawn(move || {
                while !sh.stopping.load(Relaxed) {
                    let deadline = now_ns() + every * 1_000_000;
                    while now_ns() < deadline {
                        if sh.stopping.load(Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    sh.poll_once();
                }
            }));
        }
        Ok(Engine {
            shared,
            rx: pub_rx,
            threads,
        })
    }

    pub fn counters(&self) -> Counters {
        let c = &self.shared.counters;
        Counters {
            notify_events: c.notify_events.load(Relaxed),
            paths: c.paths.load(Relaxed),
            filtered: c.filtered.load(Relaxed),
            rescans: c.rescans.load(Relaxed),
            errors: c.errors.load(Relaxed),
            poll_cycles: c.poll_cycles.load(Relaxed),
            poll_triggered: c.poll_triggered.load(Relaxed),
            watch_restarts: c.watch_restarts.load(Relaxed),
        }
    }

    pub fn reconcile_all(&self) {
        for r in self.shared.routes.read().unwrap().iter() {
            let _ = r.tx.send(WMsg::Reconcile { kind: "reconciled" });
        }
    }

    pub fn worktree_names(&self) -> Vec<String> {
        self.shared
            .routes
            .read()
            .unwrap()
            .iter()
            .map(|r| r.name.clone())
            .collect()
    }

    /// Drop the OS watcher (simulates a watcher crash/restart) without stopping workers.
    pub fn kill_watcher(&self) {
        *self.shared.watcher.lock().unwrap() = None;
    }

    pub fn restart_watcher(&self) -> Result<()> {
        let sh = self.shared.clone();
        let mut w =
            notify::recommended_watcher(move |res: notify::Result<Event>| sh.on_event(res))?;
        for r in self.shared.routes.read().unwrap().iter() {
            w.watch(&r.root, RecursiveMode::Recursive)?;
        }
        *self.shared.watcher.lock().unwrap() = Some(w);
        Ok(())
    }

    /// Add then remove an unrelated watch: forces the FSEvents stream to be recreated.
    pub fn churn_watch(&self, extra: &Path) -> Result<()> {
        let mut g = self.shared.watcher.lock().unwrap();
        let w = g.as_mut().ok_or_else(|| anyhow!("no watcher"))?;
        w.watch(extra, RecursiveMode::Recursive)?;
        w.unwatch(extra)?;
        self.shared.counters.watch_restarts.fetch_add(2, Relaxed);
        Ok(())
    }

    pub fn poll_once(&self) {
        self.shared.poll_once();
    }

    pub fn stop(self) {
        self.shared.stopping.store(true, Relaxed);
        *self.shared.watcher.lock().unwrap() = None;
        for r in self.shared.routes.read().unwrap().iter() {
            let _ = r.tx.send(WMsg::Stop);
        }
        let workers: Vec<_> = self
            .shared
            .workers
            .lock()
            .unwrap()
            .drain()
            .map(|(_, h)| h)
            .collect();
        for h in workers {
            let _ = h.join();
        }
        let _ = self.shared.mgr_tx.lock().unwrap().send(String::new());
        // Closing the publisher ends the subscriber thread.
        *self.shared.publisher.lock().unwrap() = Box::new(std::io::sink());
        drop(self.rx);
        for t in self.threads {
            let _ = t.join();
        }
    }
}

#[cfg(unix)]
fn socket_pair() -> Result<(Box<dyn Write + Send>, Box<dyn std::io::Read + Send>)> {
    let (a, b) = std::os::unix::net::UnixStream::pair()?;
    Ok((Box::new(a), Box::new(b)))
}

#[cfg(not(unix))]
fn socket_pair() -> Result<(Box<dyn Write + Send>, Box<dyn std::io::Read + Send>)> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    let a = std::net::TcpStream::connect(l.local_addr()?)?;
    a.set_nodelay(true)?;
    let (b, _) = l.accept()?;
    Ok((Box::new(a), Box::new(b)))
}

impl Shared {
    fn register(self: &Arc<Self>, d: WtDesc) -> Result<()> {
        if self.routes.read().unwrap().iter().any(|r| r.name == d.name) {
            return Ok(());
        }
        let (tx, rx) = channel();
        let branch = Arc::new(Mutex::new(String::new()));
        let route = Route {
            name: d.name.clone(),
            root: d.root.clone(),
            gitdir: d.gitdir.clone(),
            tx,
            ignore: build_ignore(&d.root),
            branch: branch.clone(),
        };
        self.routes.write().unwrap().push(route);
        let sh = self.clone();
        let name = d.name.clone();
        let h = std::thread::spawn(move || worker(sh, d, rx, branch));
        self.workers.lock().unwrap().insert(name.clone(), h);
        // The main root watch already covers `.git`; every worktree root is watched recursively.
        if let Some(w) = self.watcher.lock().unwrap().as_mut() {
            let root = self
                .routes
                .read()
                .unwrap()
                .iter()
                .find(|r| r.name == name)
                .unwrap()
                .root
                .clone();
            w.watch(&root, RecursiveMode::Recursive)?;
            self.counters.watch_restarts.fetch_add(1, Relaxed);
        }
        Ok(())
    }

    fn deregister(&self, name: &str) {
        let mut routes = self.routes.write().unwrap();
        if let Some(pos) = routes.iter().position(|r| r.name == name) {
            let r = routes.remove(pos);
            drop(routes);
            if let Some(w) = self.watcher.lock().unwrap().as_mut() {
                let _ = w.unwatch(&r.root);
                self.counters.watch_restarts.fetch_add(1, Relaxed);
            }
            let _ = r.tx.send(WMsg::Stop);
            if let Some(h) = self.workers.lock().unwrap().remove(name) {
                let _ = h.join();
            }
        }
    }

    fn check_membership(self: &Arc<Self>, name: &str) {
        if name.is_empty() || name == "main" {
            return;
        }
        let dir = self.common.join("worktrees").join(name);
        let known = self.routes.read().unwrap().iter().any(|r| r.name == name);
        if dir.exists() && !known {
            // `git worktree add` creates the admin dir before checking out: wait until the link is valid.
            let deadline = now_ns() + 10_000_000_000;
            while now_ns() < deadline {
                if let Some(d) = validate_linked(&self.common, name) {
                    let _ = self.register(d);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        } else if known {
            let root = self
                .routes
                .read()
                .unwrap()
                .iter()
                .find(|r| r.name == name)
                .map(|r| r.root.clone());
            let gone = !dir.exists() || root.map(|r| !r.exists()).unwrap_or(true);
            if gone {
                let t_recv = now_ns();
                self.deregister(name);
                let p = Pub {
                    wt: name.to_string(),
                    kind: "removed".into(),
                    t_recv,
                    t_flush: t_recv,
                    t_computed: now_ns(),
                    t_persisted: now_ns(),
                    ..Default::default()
                };
                self.publish(p);
            }
        }
    }

    fn on_event(&self, res: notify::Result<Event>) {
        let t_recv = now_ns();
        let ev = match res {
            Ok(e) => e,
            Err(_) => {
                self.counters.errors.fetch_add(1, Relaxed);
                return;
            }
        };
        self.counters.notify_events.fetch_add(1, Relaxed);
        if self.drop_events.load(Relaxed) {
            return;
        }
        if ev.need_rescan() {
            self.counters.rescans.fetch_add(1, Relaxed);
            for r in self.routes.read().unwrap().iter() {
                let _ = r.tx.send(WMsg::Reconcile { kind: "reconciled" });
            }
            return;
        }
        if let Some((prefix, seen, count)) = self.probe.lock().unwrap().as_mut() {
            for p in &ev.paths {
                if p.starts_with(&*prefix) {
                    seen.insert(p.clone());
                    *count += 1;
                }
            }
        }
        let routes = self.routes.read().unwrap();
        let mut per_wt: HashMap<usize, Vec<PathBuf>> = HashMap::new();
        for p in &ev.paths {
            self.counters.paths.fetch_add(1, Relaxed);
            match self.route(&routes, p) {
                Some(targets) => {
                    for i in targets {
                        per_wt.entry(i).or_default().push(p.clone());
                    }
                }
                None => {
                    self.counters.filtered.fetch_add(1, Relaxed);
                }
            }
        }
        for (i, paths) in per_wt {
            let _ = routes[i].tx.send(WMsg::Paths { t_recv, paths });
        }
    }

    /// Map a path to the worktrees it affects. `None` means filtered (objects, ignored, unknown).
    fn route(&self, routes: &[Route], p: &Path) -> Option<Vec<usize>> {
        if let Ok(rel) = p.strip_prefix(&self.common) {
            let comps: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            let first = comps.first().map(|s| s.as_str()).unwrap_or("");
            return match first {
                "objects" | "config" | "description" | "hooks" | "info" | "gitraptor" => None,
                "worktrees" => {
                    let name = comps.get(1)?;
                    if comps.len() <= 2
                        || comps
                            .get(2)
                            .map(|s| s == "gitdir" || s == "index")
                            .unwrap_or(false)
                    {
                        let _ = self.mgr_tx.lock().unwrap().send(name.clone());
                    }
                    let i = routes.iter().position(|r| &r.name == name)?;
                    Some(vec![i])
                }
                "refs" | "logs" | "packed-refs" => {
                    let s = comps.join("/");
                    if s == "logs/HEAD" {
                        return Some(vec![0]);
                    }
                    let branch = s
                        .strip_prefix("refs/heads/")
                        .or_else(|| s.strip_prefix("logs/refs/heads/"));
                    let idx: Vec<usize> = match branch {
                        Some(b) if b == self.cfg.base_branch => (0..routes.len()).collect(),
                        Some(b) => routes
                            .iter()
                            .enumerate()
                            .filter(|(_, r)| *r.branch.lock().unwrap() == b)
                            .map(|(i, _)| i)
                            .collect(),
                        None if s == "packed-refs" => (0..routes.len()).collect(),
                        None => Vec::new(),
                    };
                    if idx.is_empty() { None } else { Some(idx) }
                }
                _ => routes
                    .iter()
                    .position(|r| r.name == "main")
                    .map(|i| vec![i]),
            };
        }
        let (i, r) = routes
            .iter()
            .enumerate()
            .filter(|(_, r)| p.starts_with(&r.root))
            .max_by_key(|(_, r)| r.root.as_os_str().len())?;
        if p == r.root {
            let _ = self.mgr_tx.lock().unwrap().send(r.name.clone());
            return Some(vec![i]);
        }
        let rel = p.strip_prefix(&r.root).ok()?;
        if rel == Path::new(".git") {
            return None;
        }
        if r.ignore
            .matched_path_or_any_parents(p, p.is_dir())
            .is_ignore()
        {
            return None;
        }
        Some(vec![i])
    }

    fn publish(&self, mut p: Pub) {
        p.seq = self.seq.fetch_add(1, Relaxed);
        let mut w = self.publisher.lock().unwrap();
        p.t_published = now_ns();
        let mut line = serde_json::to_string(&p).unwrap();
        line.push('\n');
        let _ = w.write_all(line.as_bytes());
        let _ = w.flush();
    }

    fn poll_once(&self) {
        self.counters.poll_cycles.fetch_add(1, Relaxed);
        let routes = self.routes.read().unwrap();
        for r in routes.iter() {
            let fp = poll_fingerprint(&self.common, &r.gitdir);
            let known = self.fingerprints.lock().unwrap().get(&r.name).cloned();
            if known.as_ref() != Some(&fp) {
                self.counters.poll_triggered.fetch_add(1, Relaxed);
                let _ = r.tx.send(WMsg::Reconcile {
                    kind: "poll-reconciled",
                });
            }
        }
    }
}

/// Cheap fingerprint for the backup poll (ADR-GRP-010 § 5): HEAD, the ref it points to,
/// size+mtime of `index` and `packed-refs`, and operation markers.
pub fn poll_fingerprint(common: &Path, gitdir: &Path) -> String {
    let head = std::fs::read_to_string(gitdir.join("HEAD")).unwrap_or_default();
    let target = head
        .trim()
        .strip_prefix("ref: ")
        .map(|r| std::fs::read_to_string(common.join(r)).unwrap_or_default())
        .unwrap_or_default();
    let st = |p: PathBuf| {
        std::fs::metadata(p)
            .map(|m| format!("{:?}", sig(&m)))
            .unwrap_or_default()
    };
    let mut s = format!(
        "{}|{}|{}|{}",
        head.trim(),
        target.trim(),
        st(gitdir.join("index")),
        st(common.join("packed-refs"))
    );
    for m in OPS {
        if gitdir.join(m).exists() {
            s.push_str(m);
        }
    }
    s
}

const OPS: [&str; 6] = [
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
];

struct IdxEnt {
    mtime_s: u32,
    mtime_ns: u32,
    size: u32,
    oid: gix::ObjectId,
}

struct WtState {
    d: WtDesc,
    repo: gix::Repository,
    index: HashMap<BString, IdxEnt>,
    index_sig: Option<FileSig>,
    head_ref: String,
    head_oid: Option<gix::ObjectId>,
    tree_of: Option<gix::ObjectId>,
    tree: HashMap<BString, gix::ObjectId>,
    staged: usize,
    ab: (i64, i64),
    ab_key: Option<(gix::ObjectId, gix::ObjectId)>,
    ops: Vec<String>,
    stat_cache: HashMap<BString, (FileSig, gix::ObjectId)>,
    dirty: BTreeMap<BString, char>,
    ignore: Gitignore,
    base: String,
}

type Change = (BString, char, Option<gix::ObjectId>);

impl WtState {
    fn open(d: WtDesc, base: &str) -> Result<WtState> {
        let repo =
            gix::open_opts(&d.root, gix::open::Options::isolated()).map_err(|e| anyhow!("{e}"))?;
        Ok(WtState {
            ignore: build_ignore(&d.root),
            d,
            repo,
            index: HashMap::new(),
            index_sig: None,
            head_ref: String::new(),
            head_oid: None,
            tree_of: None,
            tree: HashMap::new(),
            staged: 0,
            ab: (-1, -1),
            ab_key: None,
            ops: Vec::new(),
            stat_cache: HashMap::new(),
            dirty: BTreeMap::new(),
            base: base.to_string(),
        })
    }

    fn index_path(&self) -> PathBuf {
        self.d.gitdir.join("index")
    }

    fn load_index(&mut self) -> Result<bool> {
        let m = std::fs::metadata(self.index_path())?;
        let s = sig(&m);
        if Some(s) == self.index_sig {
            return Ok(false);
        }
        let f = gix::index::File::at(
            self.index_path(),
            gix::hash::Kind::Sha1,
            true,
            Default::default(),
        )
        .map_err(|e| anyhow!("{e:?}"))?;
        self.index.clear();
        for e in f.entries() {
            self.index.insert(
                e.path(&f).to_owned(),
                IdxEnt {
                    mtime_s: e.stat.mtime.secs,
                    mtime_ns: e.stat.mtime.nsecs,
                    size: e.stat.size,
                    oid: e.id,
                },
            );
        }
        self.index_sig = Some(s);
        Ok(true)
    }

    /// Returns (index_changed, head_changed).
    fn refresh_meta(&mut self) -> Result<(bool, bool)> {
        let idx = self.load_index()?;
        let head = self.repo.head().map_err(|e| anyhow!("{e}"))?;
        let name = head
            .referent_name()
            .map(|n| n.as_bstr().to_string())
            .unwrap_or_else(|| "(detached)".into());
        let oid = head.id().map(|i| i.detach());
        let head_changed = oid != self.head_oid || name != self.head_ref;
        self.head_ref = name;
        self.head_oid = oid;
        if head_changed && let Some(oid) = oid {
            let commit = self.repo.find_commit(oid).map_err(|e| anyhow!("{e}"))?;
            let tree = commit.tree().map_err(|e| anyhow!("{e}"))?;
            if Some(tree.id) != self.tree_of {
                let mut rec = gix::traverse::tree::Recorder::default();
                tree.traverse()
                    .breadthfirst(&mut rec)
                    .map_err(|e| anyhow!("{e}"))?;
                self.tree = rec
                    .records
                    .into_iter()
                    .filter(|e| !e.mode.is_tree())
                    .map(|e| (e.filepath, e.oid))
                    .collect();
                self.tree_of = Some(tree.id);
            }
        }
        if idx || head_changed {
            self.staged = self
                .index
                .iter()
                .filter(|(p, e)| self.tree.get(*p) != Some(&e.oid))
                .count()
                + self
                    .tree
                    .keys()
                    .filter(|p| !self.index.contains_key(*p))
                    .count();
        }
        self.ops = OPS
            .iter()
            .filter(|m| self.d.gitdir.join(m).exists())
            .map(|s| s.to_string())
            .collect();
        self.ahead_behind();
        Ok((idx, head_changed))
    }

    /// Ahead/behind against the base branch, only when one of the tips moved (cache by pair).
    /// Uses the allowlisted `git rev-list --left-right --count` (ADR-GRP-009 § 3).
    fn ahead_behind(&mut self) {
        let Some(head) = self.head_oid else { return };
        let Ok(base) = self
            .repo
            .rev_parse_single(format!("refs/heads/{}", self.base).as_str())
        else {
            return;
        };
        let key = (base.detach(), head);
        if self.ab_key == Some(key) {
            return;
        }
        let out = std::process::Command::new("git")
            .current_dir(&self.d.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_GLOBAL", crate::repo::NULL_DEVICE)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "rev-list",
                "--left-right",
                "--count",
                &format!("{}...{}", key.0, key.1),
            ])
            .output();
        if let Ok(o) = out {
            let s = String::from_utf8_lossy(&o.stdout);
            let mut it = s.split_whitespace().map(|x| x.parse::<i64>().unwrap_or(-1));
            self.ab = (it.next().unwrap_or(-1), it.next().unwrap_or(-1));
            self.ab_key = Some(key);
        }
    }

    fn is_ignored(&self, abs: &Path, is_dir: bool) -> bool {
        self.ignore
            .matched_path_or_any_parents(abs, is_dir)
            .is_ignore()
    }

    /// Status of one path relative to the index, using the stat cache to avoid re-hashing.
    fn status_of(&mut self, rel: &BString, abs: &Path) -> Option<(char, Option<gix::ObjectId>)> {
        let meta = std::fs::symlink_metadata(abs);
        let entry = self.index.get(rel);
        match (meta, entry) {
            (Err(_), Some(_)) => Some(('D', None)),
            (Err(_), None) => None,
            (Ok(m), _) if m.is_dir() => None,
            (Ok(m), Some(e)) => {
                let s = sig(&m);
                let racy = self
                    .index_sig
                    .map(|i| (s.mtime_s, s.mtime_ns) >= (i.mtime_s, i.mtime_ns))
                    .unwrap_or(true);
                if !racy
                    && s.len == e.size as u64
                    && s.mtime_s as u32 == e.mtime_s
                    && s.mtime_ns as u32 == e.mtime_ns
                {
                    return None;
                }
                let oid = match self.stat_cache.get(rel) {
                    Some((cs, oid)) if *cs == s => *oid,
                    _ => {
                        let oid = hash_file(abs)?;
                        self.stat_cache.insert(rel.clone(), (s, oid));
                        oid
                    }
                };
                if oid == e.oid {
                    None
                } else {
                    Some(('M', Some(oid)))
                }
            }
            (Ok(m), None) => {
                if self.is_ignored(abs, false) {
                    None
                } else {
                    let s = sig(&m);
                    let oid = match self.stat_cache.get(rel) {
                        Some((cs, oid)) if *cs == s => Some(*oid),
                        _ => hash_file(abs).inspect(|o| {
                            self.stat_cache.insert(rel.clone(), (s, *o));
                        }),
                    };
                    Some(('?', oid))
                }
            }
        }
    }

    fn apply(&mut self, rel: BString, abs: &Path, changes: &mut Vec<Change>) {
        let st = self.status_of(&rel, abs);
        let prev = self.dirty.get(&rel).copied();
        match st {
            Some((c, oid)) => {
                if prev != Some(c) || c != 'D' {
                    changes.push((rel.clone(), c, oid));
                }
                self.dirty.insert(rel, c);
            }
            None => {
                if prev.is_some() {
                    changes.push((rel.clone(), ' ', None));
                    self.dirty.remove(&rel);
                }
            }
        }
    }

    fn rel(&self, abs: &Path) -> Option<BString> {
        let r = abs.strip_prefix(&self.d.root).ok()?;
        Some(BString::from(r.to_string_lossy().replace('\\', "/")))
    }

    /// Re-check every index entry (after an index change, or as part of reconciliation).
    fn full_index_compare(&mut self, changes: &mut Vec<Change>) {
        let paths: Vec<BString> = self.index.keys().cloned().collect();
        for rel in paths {
            let abs = self.d.root.join(rel.to_str_lossy().as_ref());
            self.apply(rel, &abs, changes);
        }
        // Untracked entries that are now tracked, or gone.
        let stale: Vec<BString> = self
            .dirty
            .iter()
            .filter(|(p, c)| {
                **c == '?'
                    && (self.index.contains_key(*p)
                        || !self.d.root.join(p.to_str_lossy().as_ref()).exists())
            })
            .map(|(p, _)| p.clone())
            .collect();
        for p in stale {
            let abs = self.d.root.join(p.to_str_lossy().as_ref());
            self.apply(p, &abs, changes);
        }
    }

    fn walk_untracked(&mut self, dir: &Path, changes: &mut Vec<Change>) {
        let walker = ignore::WalkBuilder::new(dir)
            .hidden(false)
            .parents(true)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(false)
            .require_git(false)
            .filter_entry(|e| e.file_name() != ".git")
            .build();
        for e in walker.flatten() {
            if e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && let Some(rel) = self.rel(e.path())
                && !self.index.contains_key(&rel)
            {
                let abs = e.path().to_path_buf();
                self.apply(rel, &abs, changes);
            }
        }
    }

    fn reconcile(&mut self) -> Result<Vec<Change>> {
        self.index_sig = None;
        self.head_oid = None;
        self.refresh_meta()?;
        let mut changes = Vec::new();
        self.full_index_compare(&mut changes);
        let root = self.d.root.clone();
        self.walk_untracked(&root, &mut changes);
        Ok(changes)
    }

    fn incremental(&mut self, paths: &HashSet<PathBuf>, full: &mut bool) -> Result<Vec<Change>> {
        let mut changes = Vec::new();
        let meta = paths
            .iter()
            .any(|p| !p.starts_with(&self.d.root) || p.starts_with(self.d.root.join(".git")));
        if meta {
            let (idx, _) = self.refresh_meta()?;
            if idx {
                *full = true;
                self.full_index_compare(&mut changes);
            }
        }
        for p in paths {
            if !p.starts_with(&self.d.root) || p.starts_with(self.d.root.join(".git")) {
                continue;
            }
            let Some(rel) = self.rel(p) else { continue };
            match std::fs::symlink_metadata(p) {
                Ok(m) if m.is_dir() => {
                    if !self.is_ignored(p, true) {
                        let p = p.clone();
                        self.walk_untracked(&p, &mut changes);
                    }
                }
                Ok(_) => self.apply(rel, p, &mut changes),
                Err(_) => {
                    if self.index.contains_key(&rel) || self.dirty.contains_key(&rel) {
                        self.apply(rel, p, &mut changes);
                    } else {
                        // A removed directory: re-check everything known below it.
                        let mut prefix = rel.clone();
                        prefix.push(b'/');
                        let below: Vec<BString> = self
                            .dirty
                            .keys()
                            .filter(|k| k.starts_with(prefix.as_slice()))
                            .cloned()
                            .chain(
                                self.index
                                    .keys()
                                    .filter(|k| k.starts_with(prefix.as_slice()))
                                    .cloned(),
                            )
                            .collect();
                        for k in below {
                            let abs = self.d.root.join(k.to_str_lossy().as_ref());
                            self.apply(k, &abs, &mut changes);
                        }
                    }
                }
            }
        }
        Ok(changes)
    }

    fn snapshot(&self, kind: &str, changes: &[Change]) -> Pub {
        Pub {
            wt: self.d.name.clone(),
            kind: kind.into(),
            head_ref: self.head_ref.clone(),
            head_oid: self.head_oid.map(|o| o.to_string()).unwrap_or_default(),
            index_sig: self
                .index_sig
                .map(|s| format!("{}:{}.{}", s.len, s.mtime_s, s.mtime_ns))
                .unwrap_or_default(),
            dirty: self.dirty.len(),
            staged: self.staged,
            ahead: self.ab.1,
            behind: self.ab.0,
            ops: self.ops.clone(),
            changed_total: changes.len(),
            changed: changes
                .iter()
                .take(64)
                .map(|(p, c, o)| {
                    (
                        p.to_string(),
                        *c,
                        o.map(|o| o.to_string()).unwrap_or_default(),
                    )
                })
                .collect(),
            ..Default::default()
        }
    }
}

fn persist(sh: &Shared, p: &Pub, changes: &[Change]) -> Result<()> {
    let mut db = sh.store.lock().unwrap();
    let tx = db.transaction()?;
    tx.execute(
        "INSERT INTO wt_state(name, state) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET state = excluded.state",
        rusqlite::params![p.wt, format!("{}|{}|{}|{}", p.head_ref, p.head_oid, p.dirty, p.staged)],
    )?;
    {
        let mut st = tx.prepare_cached(
            "INSERT INTO events(wt, batch, path, status, t) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (path, c, _) in changes {
            st.execute(rusqlite::params![
                p.wt,
                p.t_flush as i64,
                path.to_string(),
                c.to_string(),
                p.t_flush as i64
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn worker(sh: Arc<Shared>, d: WtDesc, rx: Receiver<WMsg>, branch: Arc<Mutex<String>>) {
    let name = d.name.clone();
    let mut st = match WtState::open(d, &sh.cfg.base_branch) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("worker {name}: {e}");
            return;
        }
    };
    let window = sh.cfg.window_ms * 1_000_000;
    let mut first_kind: Option<&'static str> = Some("added");
    let mut pending: Option<WMsg> = None;
    loop {
        let (t_recv, mut acc, mut n, mut reconcile) = if let Some(kind) = first_kind.take() {
            (now_ns(), HashSet::new(), 0usize, Some(kind))
        } else {
            let msg = match pending.take() {
                Some(m) => m,
                None => match rx.recv() {
                    Ok(m) => m,
                    Err(_) => return,
                },
            };
            match msg {
                WMsg::Stop => return,
                WMsg::Reconcile { kind } => (now_ns(), HashSet::new(), 0, Some(kind)),
                WMsg::Paths { t_recv, paths } => {
                    (t_recv, paths.into_iter().collect::<HashSet<_>>(), 1, None)
                }
            }
        };
        // Fixed window: closes at first event + W. Sliding: each event pushes it back.
        if reconcile.is_none() {
            let mut deadline = t_recv + window;
            loop {
                let now = now_ns();
                if now >= deadline {
                    // Drain what is already queued without waiting.
                    match rx.try_recv() {
                        Ok(WMsg::Paths { t_recv: t, paths }) if window == 0 || t <= deadline => {
                            acc.extend(paths);
                            n += 1;
                            continue;
                        }
                        Ok(other) => {
                            pending = Some(other);
                        }
                        Err(_) => {}
                    }
                    break;
                }
                match rx.recv_timeout(Duration::from_nanos(deadline - now)) {
                    Ok(WMsg::Paths { paths, .. }) => {
                        acc.extend(paths);
                        n += 1;
                        if sh.cfg.sliding {
                            deadline = now_ns() + window;
                        }
                    }
                    Ok(WMsg::Reconcile { kind }) => {
                        reconcile = Some(kind);
                        break;
                    }
                    Ok(WMsg::Stop) => return,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
        let t_flush = now_ns();
        let mut full = false;
        let (kind, res) = match reconcile {
            Some(k) => {
                full = true;
                (k, st.reconcile())
            }
            None => ("state", st.incremental(&acc, &mut full)),
        };
        let changes = match res {
            Ok(c) => c,
            Err(e) => {
                // The worktree may be disappearing; let the membership check decide.
                let _ = sh.mgr_tx.lock().unwrap().send(st.d.name.clone());
                if !st.d.root.exists() {
                    return;
                }
                eprintln!("worker {}: {e}", st.d.name);
                continue;
            }
        };
        *branch.lock().unwrap() = st
            .head_ref
            .strip_prefix("refs/heads/")
            .unwrap_or("")
            .to_string();
        let t_computed = now_ns();
        let mut p = st.snapshot(kind, &changes);
        p.events_in_batch = n;
        p.t_recv = t_recv;
        p.t_flush = t_flush;
        p.t_computed = t_computed;
        p.full_recompute = full;
        if let Err(e) = persist(&sh, &p, &changes) {
            eprintln!("persist: {e}");
        }
        p.t_persisted = now_ns();
        sh.publish(p);
        let fp = poll_fingerprint(&sh.common, &st.d.gitdir);
        sh.fingerprints
            .lock()
            .unwrap()
            .insert(st.d.name.clone(), fp);
    }
}

/// Degraded-mode cost: full stat-based status of one worktree, steady state (stat cache warm).
pub fn degraded_steady(d: &WtDesc, base: &str, iters: usize) -> Result<Vec<f64>> {
    let mut st = WtState::open(d.clone(), base)?;
    let _ = st.reconcile()?;
    let mut out = Vec::new();
    for _ in 0..iters {
        let t = now_ns();
        st.index_sig = None;
        st.refresh_meta()?;
        let mut changes = Vec::new();
        st.full_index_compare(&mut changes);
        let root = st.d.root.clone();
        st.walk_untracked(&root, &mut changes);
        out.push(crate::util::ms(now_ns() - t));
    }
    Ok(out)
}
