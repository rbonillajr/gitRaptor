//! Prototype of the snapshot store of ADR-TMC-001 and the capture pipeline whose stages
//! ADR-TMC-006 § 2 budgets. Four writer variants map to the ladder of ADR-TMC-006 § 5:
//!
//! - `cli`      : baseline, Git CLI with one process per step (ADR-GRP-001 as written).
//! - `fi`       : step 1, one persistent `git fast-import` per store (+ persistent `cat-file`).
//! - `fi-hint`  : steps 1+2, change detection from the engine's in-memory state (hint paths
//!   plus stat cache) instead of a full `git status` walk.
//! - `gix-hint` : step 3 (+2), the store is written in-process with gitoxide.
//!
//! The user's repository is only ever read (`GIT_OPTIONAL_LOCKS=0`, no index refresh).

use crate::util::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    Cli,
    Fi,
    FiHint,
    GixHint,
}

impl Variant {
    pub fn parse(s: &str) -> Res<Self> {
        Ok(match s {
            "cli" => Variant::Cli,
            "fi" => Variant::Fi,
            "fi-hint" => Variant::FiHint,
            "gix-hint" => Variant::GixHint,
            _ => return err(format!("unknown variant {s}")),
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Variant::Cli => "cli",
            Variant::Fi => "fi",
            Variant::FiHint => "fi-hint",
            Variant::GixHint => "gix-hint",
        }
    }
    pub fn hint(self) -> bool {
        matches!(self, Variant::FiHint | Variant::GixHint)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SeedMode {
    Hardlink,
    Clone,
    Copy,
}

#[derive(Clone, Debug, Default)]
pub struct Timing {
    pub queue: Duration,
    pub detect: Duration,
    pub anchor: Duration,
    pub blobs: Duration,
    pub trees: Duration,
    pub ref_oplog: Duration,
    pub total: Duration,
    pub fast_path: bool,
    pub hashed_files: usize,
    pub hashed_bytes: u64,
}

/// A blob written by a worker thread: (worktree index, path, mode, stat, sha, size).
type Written = (usize, String, String, Option<StatKey>, String, u64);

const ZERO: &str = "0000000000000000000000000000000000000000";

struct FastImport {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    mark: u64,
}

struct CatFile {
    _child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl CatFile {
    fn exists(&mut self, sha: &str) -> Res<bool> {
        writeln!(self.stdin, "{sha}")?;
        self.stdin.flush()?;
        let mut line = String::new();
        self.stdout.read_line(&mut line)?;
        Ok(!line.trim_end().ends_with("missing"))
    }
}

/// Persistent `git update-ref --stdin` with explicit transactions (one per snapshot ref).
struct UpdateRef {
    _child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl UpdateRef {
    fn create(&mut self, name: &str, sha: &str) -> Res<()> {
        write!(self.stdin, "start\ncreate {name} {sha}\ncommit\n")?;
        self.stdin.flush()?;
        for want in ["start: ok", "commit: ok"] {
            let mut line = String::new();
            self.stdout.read_line(&mut line)?;
            if line.trim_end() != want {
                return err(format!("update-ref answered {line:?}"));
            }
        }
        Ok(())
    }
}

pub struct Writer {
    fi: Option<FastImport>,
    upd: Option<UpdateRef>,
    catfile: Option<CatFile>,
    gix: Option<gix::Repository>,
    gix_sync: Option<gix::ThreadSafeRepository>,
    anchored: HashSet<String>,
}

pub struct Shared {
    pub variant: Variant,
    pub store: PathBuf,
    pub prof: PathBuf,
    empty_wt: PathBuf,
    pub user_git: PathBuf,
    writer: Mutex<Writer>,
    oplog: Mutex<std::fs::File>,
    refs: Mutex<Vec<(String, String)>>,
    last_scope: Mutex<HashMap<String, (u64, String)>>,
    counter: AtomicU64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dirty {
    Present,
    Deleted,
}

pub struct Wt {
    pub key: String,
    pub path: PathBuf,
    index_file: PathBuf,
    head_file: PathBuf,
    itree: String,
    imap: HashMap<String, (String, String)>,
    index_stat: Option<StatKey>,
    base_index: PathBuf,
    work_index: PathBuf,
    dirty: BTreeMap<String, Dirty>,
    cache: HashMap<String, (StatKey, String, String)>,
}

/// Initialise the private bare store `<prof>/store.git` with the configuration of ADR-TMC-001 § 4.
pub fn init_store(prof: &Path) -> Res<PathBuf> {
    ensure_dir(prof)?;
    let store = prof.join("store.git");
    let mut c = git();
    c.args(["init", "-q", "--bare"]).arg(&store);
    run(c)?;
    let hooks = ensure_dir(&prof.join("nohooks"))?;
    let cfg: &[(&str, &str)] = &[
        ("gc.auto", "0"),
        ("maintenance.auto", "false"),
        ("core.logAllRefUpdates", "false"),
        ("core.hooksPath", hooks.to_str().unwrap()),
        ("core.fsync", "committed"),
        ("core.fsyncMethod", "batch"),
        // Snapshot content is often already compressed or binary: favour speed over size.
        ("pack.compression", "1"),
        ("core.bigFileThreshold", "128k"),
        // Keep fast-import's small per-snapshot packs (default: unpack < 100 objects into
        // loose objects, each with its own fsync). Maintenance consolidates them later.
        ("fastimport.unpackLimit", "0"),
        ("user.name", "GitRaptor Time Machine"),
        ("user.email", "tm@gitraptor.invalid"),
    ];
    for (k, v) in cfg {
        let mut c = git();
        c.arg("--git-dir").arg(&store).args(["config", k, v]);
        run(c)?;
    }
    std::fs::set_permissions(prof, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(store)
}

/// Seed the store with the user's packs (ADR-TMC-001 § 3). Returns the files seeded.
pub fn seed(user_git: &Path, store: &Path, mode: SeedMode) -> Res<usize> {
    let src = user_git.join("objects/pack");
    let dst = store.join("objects/pack");
    let mut n = 0;
    for e in std::fs::read_dir(&src)? {
        let p = e?.path();
        let name = p.file_name().unwrap();
        let target = dst.join(name);
        match mode {
            SeedMode::Hardlink => std::fs::hard_link(&p, &target)?,
            SeedMode::Clone => {
                let mut c = std::process::Command::new("cp");
                c.arg("-c").arg(&p).arg(&target);
                run(c)?;
            }
            SeedMode::Copy => {
                // Explicit byte copy (std::fs::copy would clone on APFS).
                let mut r = std::fs::File::open(&p)?;
                let mut w = std::fs::File::create(&target)?;
                std::io::copy(&mut r, &mut w)?;
                w.sync_all()?;
            }
        }
        n += 1;
    }
    Ok(n)
}

impl Shared {
    pub fn open(variant: Variant, prof: &Path, store: &Path, user_git: &Path) -> Res<Self> {
        let empty_wt = ensure_dir(&prof.join("empty-wt"))?;
        let mut w = Writer {
            fi: None,
            upd: None,
            catfile: None,
            gix: None,
            gix_sync: None,
            anchored: HashSet::new(),
        };
        if matches!(variant, Variant::Fi | Variant::FiHint) {
            let mut child = git()
                .arg("--git-dir")
                .arg(store)
                // --force: the scratch branch is rewritten on every snapshot.
                .args(["fast-import", "--quiet", "--force"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let stdin = BufWriter::with_capacity(1 << 20, child.stdin.take().unwrap());
            let stdout = BufReader::new(child.stdout.take().unwrap());
            w.fi = Some(FastImport {
                child,
                stdin,
                stdout,
                mark: 0,
            });
            let mut child = git()
                .arg("--git-dir")
                .arg(store)
                .args(["update-ref", "--stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let stdin = BufWriter::new(child.stdin.take().unwrap());
            let stdout = BufReader::new(child.stdout.take().unwrap());
            w.upd = Some(UpdateRef {
                _child: child,
                stdin,
                stdout,
            });
            let mut child = git()
                .arg("--git-dir")
                .arg(store)
                .args(["cat-file", "--batch-check"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let stdin = BufWriter::new(child.stdin.take().unwrap());
            let stdout = BufReader::new(child.stdout.take().unwrap());
            w.catfile = Some(CatFile {
                _child: child,
                stdin,
                stdout,
            });
        }
        if variant == Variant::GixHint {
            let repo = gix::open(store)?;
            w.gix_sync = Some(repo.clone().into_sync());
            w.gix = Some(repo);
        }
        let oplog = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(prof.join("oplog.log"))?;
        let s = Shared {
            variant,
            store: store.to_path_buf(),
            prof: prof.to_path_buf(),
            empty_wt,
            user_git: user_git.to_path_buf(),
            writer: Mutex::new(w),
            oplog: Mutex::new(oplog),
            refs: Mutex::new(Vec::new()),
            last_scope: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
        };
        *s.refs.lock().unwrap() = s.read_refs()?;
        Ok(s)
    }

    pub fn close(&self) -> Res<()> {
        let mut w = self.writer.lock().unwrap();
        if let Some(mut fi) = w.fi.take() {
            writeln!(fi.stdin, "done")?;
            fi.stdin.flush()?;
            drop(fi.stdin);
            fi.child.wait()?;
        }
        Ok(())
    }

    fn store_git(&self) -> std::process::Command {
        let mut c = git();
        c.env("GIT_DIR", &self.store);
        c
    }

    fn read_refs(&self) -> Res<Vec<(String, String)>> {
        let mut c = git();
        c.arg("--git-dir").arg(&self.user_git).args([
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/heads",
            "refs/stash",
        ]);
        let out = String::from_utf8(run(c)?)?;
        Ok(out
            .lines()
            .filter_map(|l| {
                l.split_once(' ')
                    .map(|(a, b)| (b.to_string(), a.to_string()))
            })
            .collect())
    }

    pub fn new_wt(&self, key: &str, path: &Path) -> Res<Wt> {
        let mut c = git();
        c.current_dir(path)
            .args(["rev-parse", "--absolute-git-dir"]);
        let gd = PathBuf::from(trim(&run(c)?));
        let mut wt = Wt {
            key: key.to_string(),
            path: path.to_path_buf(),
            index_file: gd.join("index"),
            head_file: gd.join("HEAD"),
            itree: String::new(),
            imap: HashMap::new(),
            index_stat: None,
            base_index: self.prof.join(format!("{key}.base.index")),
            work_index: self.prof.join(format!("{key}.work.index")),
            dirty: BTreeMap::new(),
            cache: HashMap::new(),
        };
        self.rebuild_itree(&mut wt)?;
        Ok(wt)
    }

    /// Mirror the user's index into the store (tree `wt/<k>/index`), via a temporary index
    /// that lives in the profile. Never touches the user's index.
    fn rebuild_itree(&self, wt: &mut Wt) -> Res<()> {
        let st = stat_key(&wt.index_file)?;
        let mut c = git();
        c.current_dir(&wt.path).args(["ls-files", "-s", "-z"]);
        let out = run(c)?;
        let mut info = Vec::with_capacity(out.len());
        wt.imap.clear();
        for rec in out.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            let s = String::from_utf8_lossy(rec);
            let (meta, path) = s.split_once('\t').ok_or("ls-files")?;
            let mut it = meta.split(' ');
            let (mode, sha, stage) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
            if stage != "0" {
                continue;
            }
            wt.imap
                .insert(path.to_string(), (mode.to_string(), sha.to_string()));
            writeln!(info, "{mode} {sha}\t{path}")?;
        }
        let tmp = self.prof.join(format!("{}.itree.index", wt.key));
        let _ = std::fs::remove_file(&tmp);
        let mut c = self.store_git();
        c.env("GIT_INDEX_FILE", &tmp)
            .env("GIT_WORK_TREE", &self.empty_wt)
            .args(["update-index", "--index-info"]);
        run_input(c, &info)?;
        let mut c = self.store_git();
        c.env("GIT_INDEX_FILE", &tmp).arg("write-tree");
        wt.itree = trim(&run(c)?);
        if self.variant == Variant::Cli {
            let _ = std::fs::remove_file(&wt.base_index);
            for sub in ["files", "index"] {
                let mut c = self.store_git();
                c.env("GIT_INDEX_FILE", &wt.base_index)
                    .env("GIT_WORK_TREE", &self.empty_wt)
                    .args([
                        "read-tree",
                        &format!("--prefix=wt/{}/{sub}/", wt.key),
                        &wt.itree,
                    ]);
                run(c)?;
            }
            let mut c = self.store_git();
            c.env("GIT_INDEX_FILE", &wt.base_index).arg("write-tree"); // primes cache-tree
            run(c)?;
        }
        wt.index_stat = Some(st);
        Ok(())
    }

    fn status_dirty(&self, wt: &Wt) -> Res<BTreeMap<String, Dirty>> {
        let mut c = git();
        c.current_dir(&wt.path).args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ]);
        let out = run(c)?;
        let mut d = BTreeMap::new();
        for rec in out.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            let s = String::from_utf8_lossy(rec);
            match s.as_bytes()[0] {
                b'?' => {
                    d.insert(s[2..].to_string(), Dirty::Present);
                }
                b'1' => {
                    let f: Vec<&str> = s.splitn(9, ' ').collect();
                    let y = f[1].as_bytes()[1];
                    if y == b'D' {
                        d.insert(f[8].to_string(), Dirty::Deleted);
                    } else if y != b'.' {
                        d.insert(f[8].to_string(), Dirty::Present);
                    }
                }
                b'u' => {
                    let f: Vec<&str> = s.splitn(11, ' ').collect();
                    d.insert(f[10].to_string(), Dirty::Present);
                }
                _ => {}
            }
        }
        Ok(d)
    }

    fn read_head(wt: &Wt, refs: &[(String, String)]) -> Res<String> {
        let h = std::fs::read_to_string(&wt.head_file)?;
        let h = h.trim();
        Ok(match h.strip_prefix("ref: ") {
            Some(r) => refs
                .iter()
                .find(|(n, _)| n == r)
                .map(|(_, s)| s.clone())
                .unwrap_or_default(),
            None => h.to_string(),
        })
    }

    /// Take a snapshot of `scope` (worktrees). `hints`: per worktree, paths the engine reports
    /// as changed since the last capture (only used by the `*-hint` variants).
    pub fn snapshot(
        &self,
        scope: &mut [&mut Wt],
        hints: Option<&[Vec<String>]>,
        level: &str,
    ) -> Res<(String, Timing)> {
        let mut tm = Timing::default();
        let mut t = Timer::start();
        let t_all = std::time::Instant::now();

        // ---- 1. detection --------------------------------------------------------------
        let hint_mode = self.variant.hint();
        let refs = if hint_mode {
            self.refs.lock().unwrap().clone()
        } else {
            let r = self.read_refs()?;
            *self.refs.lock().unwrap() = r.clone();
            r
        };
        let detect_one = |wt: &mut Wt,
                          hint: Option<&Vec<String>>|
         -> Res<Vec<(String, String, Option<StatKey>)>> {
            if stat_key(&wt.index_file).ok() != wt.index_stat {
                self.rebuild_itree(wt)?;
            }
            if hint_mode {
                for p in hint.into_iter().flatten() {
                    match std::fs::symlink_metadata(wt.path.join(p)) {
                        Ok(_) => {
                            wt.dirty.insert(p.clone(), Dirty::Present);
                        }
                        Err(_) if wt.imap.contains_key(p) => {
                            wt.dirty.insert(p.clone(), Dirty::Deleted);
                        }
                        Err(_) => {
                            wt.dirty.remove(p);
                        }
                    }
                }
            } else {
                wt.dirty = self.status_dirty(wt)?;
            }
            // (path, mode, stat) of present entries whose stat changed since they were hashed.
            let mut to_hash = Vec::new();
            for (p, d) in &wt.dirty {
                if *d == Dirty::Present {
                    let sk = stat_key(&wt.path.join(p))?;
                    match wt.cache.get(p) {
                        Some((c, _, _)) if *c == sk => {}
                        _ => to_hash.push((p.clone(), git_mode(sk.mode).to_string(), Some(sk))),
                    }
                }
            }
            Ok(to_hash)
        };
        let mut to_hash: Vec<Vec<(String, String, Option<StatKey>)>> = Vec::new();
        if scope.len() == 1 {
            to_hash.push(detect_one(scope[0], hints.map(|h| &h[0]))?);
        } else {
            let results: Vec<Res<_>> = std::thread::scope(|s| {
                let hs: Vec<_> = scope
                    .iter_mut()
                    .enumerate()
                    .map(|(i, wt)| {
                        let h = hints.map(|h| &h[i]);
                        let f = &detect_one;
                        s.spawn(move || f(wt, h))
                    })
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap()).collect()
            });
            for r in results {
                to_hash.push(r?);
            }
        }
        let mut heads = Vec::new();
        for wt in scope.iter() {
            heads.push(Self::read_head(wt, &refs)?);
        }
        let scope_key: String = scope
            .iter()
            .map(|w| w.key.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let fp = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            refs.hash(&mut h);
            heads.hash(&mut h);
            for (i, wt) in scope.iter().enumerate() {
                wt.itree.hash(&mut h);
                for (p, d) in &wt.dirty {
                    p.hash(&mut h);
                    (*d == Dirty::Present).hash(&mut h);
                    if let Some((sk, _, _)) = wt.cache.get(p) {
                        sk.mtime_ns.hash(&mut h);
                        sk.size.hash(&mut h);
                        sk.ino.hash(&mut h);
                    }
                }
                to_hash[i].len().hash(&mut h);
            }
            h.finish()
        };
        tm.detect = t.lap();

        let id = format!(
            "{}-{}",
            std::process::id(),
            self.counter.fetch_add(1, Ordering::SeqCst)
        );
        // Fast path: nothing changed since the last capture of this scope.
        let all_clean = to_hash.iter().all(|v| v.is_empty());
        if all_clean
            && let Some((last_fp, last_commit)) =
                self.last_scope.lock().unwrap().get(&scope_key).cloned()
            && last_fp == fp
        {
            tm.fast_path = true;
            self.oplog_row(&id, &last_commit, level, true)?;
            tm.ref_oplog = t.lap();
            tm.total = t_all.elapsed();
            return Ok((last_commit, tm));
        }

        // ---- writer section (one writer per store) --------------------------------------
        let mut w = self.writer.lock().unwrap();
        tm.queue = t.lap();

        // ---- 2. anchoring of commits referenced by refs/HEADs -------------------------
        let mut parents: Vec<String> = Vec::new();
        for s in heads.iter().chain(refs.iter().map(|(_, s)| s)) {
            if !s.is_empty() && !parents.contains(s) {
                parents.push(s.clone());
            }
        }
        let missing = self.anchor_check(&mut w, &parents)?;
        if !missing.is_empty() {
            self.anchor_copy(&missing)?;
        }
        w.anchored.extend(parents.iter().cloned());
        tm.anchor = t.lap();

        // ---- 3..5 per variant ---------------------------------------------------------
        let mut meta = format!("snapshot {id}\nlevel {level}\nscope {scope_key}\n");
        for (wt, h) in scope.iter().zip(&heads) {
            meta.push_str(&format!("HEAD {} {h}\n", wt.key));
        }
        for (n, s) in &refs {
            meta.push_str(&format!("ref {s} {n}\n"));
        }
        let msg = format!("tm snapshot {id} ({level})\n");
        let commit = match self.variant {
            Variant::Cli => {
                self.write_cli(scope, &to_hash, &meta, &msg, &parents, &id, &mut tm, &mut t)?
            }
            Variant::Fi | Variant::FiHint => self.write_fi(
                &mut w, scope, &to_hash, &meta, &msg, &parents, &id, &mut tm, &mut t,
            )?,
            Variant::GixHint => self.write_gix(
                &w, scope, &to_hash, &meta, &msg, &parents, &id, &mut tm, &mut t,
            )?,
        };
        drop(w);
        self.oplog_row(&id, &commit, level, false)?;
        tm.ref_oplog += t.lap();
        tm.total = t_all.elapsed();
        self.last_scope
            .lock()
            .unwrap()
            .insert(scope_key, (fp, commit.clone()));
        Ok((commit, tm))
    }

    fn anchor_check(&self, w: &mut Writer, tips: &[String]) -> Res<Vec<String>> {
        let mut missing = Vec::new();
        match self.variant {
            Variant::Cli => {
                // Baseline: one cat-file process per snapshot, no memory between snapshots.
                let mut c = self.store_git();
                c.args(["cat-file", "--batch-check"]);
                let out =
                    String::from_utf8(run_input(c, format!("{}\n", tips.join("\n")).as_bytes())?)?;
                for (l, tip) in out.lines().zip(tips) {
                    if l.ends_with("missing") {
                        missing.push(tip.clone());
                    }
                }
            }
            _ => {
                for tip in tips {
                    if w.anchored.contains(tip) {
                        continue;
                    }
                    let present = match (&mut w.catfile, &w.gix) {
                        (Some(cf), _) => cf.exists(tip)?,
                        (_, Some(repo)) => {
                            repo.has_object(gix::ObjectId::from_hex(tip.as_bytes())?)
                        }
                        _ => false,
                    };
                    if !present {
                        missing.push(tip.clone());
                    }
                }
            }
        }
        Ok(missing)
    }

    /// Copy the objects of commits the store does not have yet (read-only on the user repo).
    fn anchor_copy(&self, missing: &[String]) -> Res<()> {
        let mut c = git();
        c.arg("--git-dir")
            .arg(&self.user_git)
            .args(["pack-objects", "--revs", "--stdout", "-q"]);
        let pack = run_input(c, format!("{}\n", missing.join("\n")).as_bytes())?;
        let mut c = self.store_git();
        c.args(["index-pack", "--stdin", "--fix-thin"]);
        run_input(c, &pack)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn write_cli(
        &self,
        scope: &mut [&mut Wt],
        to_hash: &[Vec<(String, String, Option<StatKey>)>],
        meta: &str,
        msg: &str,
        parents: &[String],
        id: &str,
        tm: &mut Timing,
        t: &mut Timer,
    ) -> Res<String> {
        // blobs: one `hash-object -w --no-filters --stdin-paths` per worktree (raw bytes, no filters).
        let meta_file = self.prof.join(format!("meta-{id}"));
        std::fs::write(&meta_file, meta)?;
        let mut meta_sha = String::new();
        for (i, wt) in scope.iter_mut().enumerate() {
            let mut input = String::new();
            if i == 0 {
                input.push_str(&format!("{}\n", meta_file.display()));
            }
            for (p, _, _) in &to_hash[i] {
                input.push_str(p);
                input.push('\n');
            }
            if input.is_empty() {
                continue;
            }
            let mut c = self.store_git();
            c.current_dir(&wt.path)
                .args(["hash-object", "-w", "--no-filters", "--stdin-paths"]);
            let out = String::from_utf8(run_input(c, input.as_bytes())?)?;
            let mut lines = out.lines();
            if i == 0 {
                meta_sha = lines.next().ok_or("meta sha")?.to_string();
            }
            for ((p, mode, sk), sha) in to_hash[i].iter().zip(lines) {
                tm.hashed_files += 1;
                tm.hashed_bytes += sk.map_or(0, |s| s.size);
                wt.cache
                    .insert(p.clone(), (sk.unwrap(), sha.to_string(), mode.clone()));
            }
        }
        let _ = std::fs::remove_file(&meta_file);
        tm.blobs = t.lap();

        // trees: copy the primed base index, overlay the dirty entries, write-tree.
        let single = scope.len() == 1;
        let mut subtrees = Vec::new();
        for wt in scope.iter() {
            std::fs::copy(&wt.base_index, &wt.work_index)?;
            let mut info = String::new();
            for (p, d) in &wt.dirty {
                match d {
                    Dirty::Present => {
                        let (_, sha, mode) = wt.cache.get(p).ok_or("cache miss")?;
                        info.push_str(&format!("{mode} {sha}\twt/{}/files/{p}\n", wt.key));
                    }
                    Dirty::Deleted => {
                        info.push_str(&format!("0 {ZERO}\twt/{}/files/{p}\n", wt.key))
                    }
                }
            }
            if single {
                info.push_str(&format!("100644 {meta_sha}\tmeta\n"));
            }
            let mut c = self.store_git();
            c.env("GIT_INDEX_FILE", &wt.work_index)
                .env("GIT_WORK_TREE", &self.empty_wt)
                .args(["update-index", "--index-info"]);
            run_input(c, info.as_bytes())?;
            let mut c = self.store_git();
            c.env("GIT_INDEX_FILE", &wt.work_index).arg("write-tree");
            if !single {
                c.arg(format!("--prefix=wt/{}/", wt.key));
            }
            subtrees.push((wt.key.clone(), trim(&run(c)?)));
        }
        let root = if single {
            subtrees[0].1.clone()
        } else {
            let mut wt_tree = String::new();
            for (k, s) in &subtrees {
                wt_tree.push_str(&format!("040000 tree {s}\t{k}\n"));
            }
            let mut c = self.store_git();
            c.arg("mktree");
            let wt_sha = trim(&run_input(c, wt_tree.as_bytes())?);
            let mut c = self.store_git();
            c.arg("mktree");
            trim(&run_input(
                c,
                format!("100644 blob {meta_sha}\tmeta\n040000 tree {wt_sha}\twt\n").as_bytes(),
            )?)
        };
        let mut c = self.store_git();
        c.args(["commit-tree", &root, "-m", msg]);
        for p in parents {
            c.args(["-p", p]);
        }
        let commit = trim(&run(c)?);
        tm.trees = t.lap();

        let mut c = self.store_git();
        c.args(["update-ref", &format!("refs/tm/snap/{id}"), &commit, ""]);
        run(c)?;
        tm.ref_oplog = t.lap();
        Ok(commit)
    }

    #[allow(clippy::too_many_arguments)]
    fn write_fi(
        &self,
        w: &mut Writer,
        scope: &mut [&mut Wt],
        to_hash: &[Vec<(String, String, Option<StatKey>)>],
        meta: &str,
        msg: &str,
        parents: &[String],
        id: &str,
        tm: &mut Timing,
        t: &mut Timer,
    ) -> Res<String> {
        let fi = w.fi.as_mut().ok_or("fast-import not running")?;
        fi.mark += 1;
        let mark = fi.mark;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let o = &mut fi.stdin;
        write!(
            o,
            "commit refs/tm/fast-import\nmark :{mark}\ncommitter GitRaptor Time Machine <tm@gitraptor.invalid> {now} +0000\ndata {}\n{msg}",
            msg.len()
        )?;
        if let Some((first, rest)) = parents.split_first() {
            writeln!(o, "from {first}")?;
            for p in rest {
                writeln!(o, "merge {p}")?;
            }
        }
        write!(
            o,
            "deleteall\nM 100644 inline meta\ndata {}\n{meta}\n",
            meta.len()
        )?;
        let mut blob_time = Duration::ZERO;
        for (i, wt) in scope.iter_mut().enumerate() {
            let k = wt.key.clone();
            writeln!(o, "M 040000 {} wt/{k}/index", wt.itree)?;
            writeln!(o, "M 040000 {} wt/{k}/files", wt.itree)?;
            let fresh: HashMap<&str, (&String, Option<StatKey>)> = to_hash[i]
                .iter()
                .map(|(p, m, s)| (p.as_str(), (m, *s)))
                .collect();
            let dirty: Vec<(String, Dirty)> =
                wt.dirty.iter().map(|(p, d)| (p.clone(), *d)).collect();
            for (p, d) in dirty {
                match d {
                    Dirty::Deleted => writeln!(o, "D wt/{k}/files/{p}")?,
                    Dirty::Present => {
                        if let Some((mode, sk)) = fresh.get(p.as_str()) {
                            let tb = std::time::Instant::now();
                            let bytes = std::fs::read(wt.path.join(&p))?;
                            let sha = blob_sha(&bytes);
                            write!(
                                o,
                                "M {mode} inline wt/{k}/files/{p}\ndata {}\n",
                                bytes.len()
                            )?;
                            o.write_all(&bytes)?;
                            o.write_all(b"\n")?;
                            tm.hashed_files += 1;
                            tm.hashed_bytes += bytes.len() as u64;
                            wt.cache
                                .insert(p.clone(), (sk.unwrap(), sha, mode.to_string()));
                            blob_time += tb.elapsed();
                        } else {
                            let (_, sha, mode) = wt.cache.get(&p).ok_or("cache miss")?;
                            writeln!(o, "M {mode} {sha} wt/{k}/files/{p}")?;
                        }
                    }
                }
            }
        }
        o.write_all(b"\n")?;
        let lap = t.lap();
        tm.blobs = blob_time;
        tm.trees = lap.saturating_sub(blob_time);
        // checkpoint: fast-import finishes the pack (fsync), then answers get-mark. Fast-import
        // only knows one scratch branch, so a checkpoint never rewrites the snapshot refs.
        write!(o, "checkpoint\nget-mark :{mark}\n")?;
        o.flush()?;
        let mut line = String::new();
        fi.stdout.read_line(&mut line)?;
        let commit = line.trim().to_string();
        if commit.len() != 40 {
            return err(format!("fast-import answered {line:?}"));
        }
        // Validity point: the snapshot ref, through a persistent `update-ref --stdin`.
        w.upd
            .as_mut()
            .ok_or("update-ref not running")?
            .create(&format!("refs/tm/snap/{id}"), &commit)?;
        tm.ref_oplog = t.lap();
        Ok(commit)
    }

    #[allow(clippy::too_many_arguments)]
    fn write_gix(
        &self,
        w: &Writer,
        scope: &mut [&mut Wt],
        to_hash: &[Vec<(String, String, Option<StatKey>)>],
        meta: &str,
        msg: &str,
        parents: &[String],
        id: &str,
        tm: &mut Timing,
        t: &mut Timer,
    ) -> Res<String> {
        use gix::objs::tree::EntryKind;
        let repo = w.gix.as_ref().ok_or("gix not open")?;
        let objects = self.store.join("objects");
        let sync_obj = |hex: &str| -> Res<()> {
            let f = std::fs::File::open(objects.join(&hex[..2]).join(&hex[2..]))?;
            fsync_plain(&f)?;
            Ok(())
        };
        let meta_id = repo.write_blob(meta.as_bytes())?.detach();
        sync_obj(&meta_id.to_string())?;
        // Blobs are independent: write them from several threads (each with its own
        // thread-local handle on the store). Loose objects get a write-out `fsync` each and one
        // full barrier at the end (same scheme as `core.fsyncMethod=batch`).
        let jobs: Vec<(usize, &String, &String, Option<StatKey>, PathBuf)> = scope
            .iter()
            .enumerate()
            .flat_map(|(i, wt)| {
                to_hash[i]
                    .iter()
                    .map(move |(p, m, sk)| (i, p, m, *sk, wt.path.join(p)))
            })
            .collect();
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(8);
        let sync = w.gix_sync.as_ref().ok_or("gix not open")?;
        let chunk = jobs.len().div_ceil(threads).max(1);
        let results: Vec<Res<Vec<Written>>> = std::thread::scope(|s| {
            let hs: Vec<_> = jobs
                .chunks(chunk)
                .map(|part| {
                    let sync_obj = &sync_obj;
                    s.spawn(move || -> Res<Vec<_>> {
                        let repo = sync.to_thread_local();
                        let mut out = Vec::new();
                        for (i, p, m, sk, full) in part {
                            let bytes = std::fs::read(full)?;
                            let hex = repo.write_blob(&bytes)?.detach().to_string();
                            sync_obj(&hex).ok();
                            out.push((
                                *i,
                                (*p).clone(),
                                (*m).clone(),
                                *sk,
                                hex,
                                bytes.len() as u64,
                            ));
                        }
                        Ok(out)
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for part in results {
            for (i, p, mode, sk, hex, len) in part? {
                tm.hashed_files += 1;
                tm.hashed_bytes += len;
                scope[i].cache.insert(p, (sk.unwrap(), hex, mode));
            }
        }
        tm.blobs = t.lap();
        let mut root = repo.edit_tree(gix::ObjectId::empty_tree(gix::hash::Kind::Sha1))?;
        for wt in scope.iter() {
            let itree = gix::ObjectId::from_hex(wt.itree.as_bytes())?;
            let mut ed = repo.edit_tree(itree)?;
            for (p, d) in &wt.dirty {
                match d {
                    Dirty::Present => {
                        let (_, sha, mode) = wt.cache.get(p).ok_or("cache miss")?;
                        let kind = match mode.as_str() {
                            "100755" => EntryKind::BlobExecutable,
                            "120000" => EntryKind::Link,
                            _ => EntryKind::Blob,
                        };
                        ed.upsert(p.as_str(), kind, gix::ObjectId::from_hex(sha.as_bytes())?)?;
                    }
                    Dirty::Deleted => {
                        ed.remove(p.as_str())?;
                    }
                }
            }
            let files = ed.write()?.detach();
            root.upsert(
                format!("wt/{}/files", wt.key).as_str(),
                EntryKind::Tree,
                files,
            )?;
            root.upsert(
                format!("wt/{}/index", wt.key).as_str(),
                EntryKind::Tree,
                itree,
            )?;
        }
        root.upsert("meta", EntryKind::Blob, meta_id)?;
        let root_id = root.write()?.detach();
        let parent_ids: Vec<gix::ObjectId> = parents
            .iter()
            .map(|p| gix::ObjectId::from_hex(p.as_bytes()))
            .collect::<Result<_, _>>()?;
        let commit = repo.new_commit(msg, root_id, parent_ids)?.id;
        let hex = commit.to_string();
        sync_obj(&hex)?;
        tm.trees = t.lap();
        let name = format!("refs/tm/snap/{id}");
        repo.reference(
            name.as_str(),
            commit,
            gix::refs::transaction::PreviousValue::MustNotExist,
            "",
        )?;
        let f = std::fs::File::open(self.store.join(&name))?;
        fsync_plain(&f)?;
        tm.ref_oplog = t.lap();
        Ok(hex)
    }

    /// Oplog row with a full durability barrier (stand-in for the SQLite row of ADR-TMC-003).
    fn oplog_row(&self, id: &str, commit: &str, level: &str, fast: bool) -> Res<()> {
        let mut f = self.oplog.lock().unwrap();
        writeln!(f, "{id} {commit} {level} complete fast_path={fast}")?;
        fsync_full(&f)?;
        Ok(())
    }
}
