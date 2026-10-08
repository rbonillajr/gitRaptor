//! `raptor status --resources` output, text and JSON (US-GRP-017,
//! ADR-GRP-015 § 4): what GitRaptor consumes, each value with its target
//! and whether it meets it. Being over a target does not change the exit
//! code: this is a view, not a gate.
//!
//! Repo paths come from the engine or the profile and are untrusted
//! (SEC-12): sanitized in the text output, JSON strings in the JSON one.

use std::collections::HashMap;
use std::fmt::Write as _;

use gitraptor_api::Untrusted;
use gitraptor_api::resources::{DiskUsage, ObservationUsage, ResourceTargets, ResourcesResult};
use serde_json::{Value, json};

use crate::i18n::t;
use crate::status::folder_of;

/// What the command shows: the engine's reading when it runs, else only
/// the disk read from the profile.
pub struct View {
    /// `Some` when the engine answered.
    pub engine: Option<EngineReading>,
    pub disk: DiskUsage,
    pub targets: ResourceTargets,
    /// Git common directory of each known repo, by id.
    pub repos: HashMap<String, String>,
}

pub struct EngineReading {
    pub pid: u32,
    pub result: ResourcesResult,
    /// Repos discovered and waiting for the developer's decision
    /// (US-GRP-020): not observed, so they cost nothing. `None` when the
    /// engine does not offer `discovery.candidates`.
    pub discovered: Option<u64>,
}

impl View {
    pub fn running(pid: u32, result: ResourcesResult, repos: HashMap<String, String>) -> Self {
        Self {
            disk: result.disk.clone(),
            targets: result.targets,
            engine: Some(EngineReading {
                pid,
                result,
                discovered: None,
            }),
            repos,
        }
    }

    /// The count of discovered repos the engine reported.
    pub fn with_discovered(mut self, discovered: Option<u64>) -> Self {
        if let Some(engine) = &mut self.engine {
            engine.discovered = discovered;
        }
        self
    }

    pub fn stopped(
        disk: DiskUsage,
        targets: ResourceTargets,
        repos: HashMap<String, String>,
    ) -> Self {
        Self {
            engine: None,
            disk,
            targets,
            repos,
        }
    }
}

const KIB: f64 = 1024.0;

/// Bytes in the largest binary unit that keeps the number at least 1.
fn bytes(n: u64) -> String {
    let n = n as f64;
    if n >= KIB * KIB * KIB {
        format!("{:.1} GiB", n / (KIB * KIB * KIB))
    } else if n >= KIB * KIB {
        format!("{:.1} MiB", n / (KIB * KIB))
    } else if n >= KIB {
        format!("{:.1} KiB", n / KIB)
    } else {
        format!("{n} B")
    }
}

fn pct(p: f64) -> String {
    format!("{p:.2} %")
}

fn window(s: u64) -> String {
    if s >= 60 {
        t("res.window-min", &[("n", &(s / 60))])
    } else {
        t("res.window-s", &[("n", &s)])
    }
}

/// `value — verdict`.
fn line(label_key: &str, value: &str, verdict: &str) -> String {
    t(
        "res.line",
        &[
            ("label", &t(label_key, &[])),
            ("value", &value),
            ("verdict", &verdict),
        ],
    )
}

fn verdict(within: Option<bool>, target: &str) -> String {
    match within {
        Some(true) => t("res.within", &[("target", &target)]),
        Some(false) => t("res.over", &[("target", &target)]),
        None => t("res.not-judged", &[("target", &target)]),
    }
}

fn na() -> String {
    t("res.not-available", &[])
}

/// The folder a repo is shown by, or its id when its path is unknown.
fn repo_name(repos: &HashMap<String, String>, id: &str) -> String {
    repos
        .get(id)
        .map(|common| folder_of(common))
        .unwrap_or_else(|| id.to_owned())
}

/// The text output.
pub fn text(view: &View) -> String {
    let mut out = String::new();
    let targets = &view.targets;
    match &view.engine {
        Some(engine) => {
            let r = &engine.result;
            let _ = writeln!(out, "{}", t("res.engine-running", &[("pid", &engine.pid)]));
            let cpu_target = format!("< {}", pct(targets.cpu_pct));
            let cpu = match (r.process.cpu.mean_pct, r.process.cpu.peak_pct) {
                (Some(mean), peak) => {
                    let value = t(
                        "res.cpu-value",
                        &[
                            ("mean", &pct(mean)),
                            ("window", &window(r.process.cpu.window_s)),
                            ("peak", &peak.map_or_else(na, pct)),
                        ],
                    );
                    line(
                        "res.cpu",
                        &value,
                        &verdict(Some(targets.cpu_within(mean)), &cpu_target),
                    )
                }
                (None, _) => line("res.cpu", &na(), &verdict(None, &cpu_target)),
            };
            let _ = writeln!(out, "{cpu}");
            let rss_target = format!("< {}", bytes(targets.rss_bytes));
            let rss = match r.process.rss_bytes {
                Some(rss) => line(
                    "res.rss",
                    &bytes(rss),
                    &verdict(Some(targets.rss_within(rss)), &rss_target),
                ),
                None => line("res.rss", &na(), &verdict(None, &rss_target)),
            };
            let _ = writeln!(out, "{rss}");
            let fds_target = targets
                .open_fds
                .map_or_else(|| t("res.no-target", &[]), |max| format!("≤ {max}"));
            let fds = match r.process.open_fds {
                Some(fds) => line(
                    "res.fds",
                    &fds.to_string(),
                    &verdict(targets.fds_within(fds), &fds_target),
                ),
                None => line("res.fds", &na(), &verdict(None, &fds_target)),
            };
            let _ = writeln!(out, "{fds}");
            let _ = writeln!(
                out,
                "{}",
                line(
                    "res.watches",
                    &t("res.roots", &[("n", &r.watches.roots)]),
                    &t("res.no-target", &[]),
                )
            );
            if let Some(backend) = &r.watches.backend {
                let _ = writeln!(
                    out,
                    "{}",
                    line(
                        "res.backend",
                        &t("res.backend-value", &[("name", &backend.as_str())]),
                        &t("res.no-target", &[]),
                    )
                );
            }
            if let Some(inotify) = &r.watches.inotify {
                let max = inotify.max_user_watches.map_or_else(na, |m| m.to_string());
                let value = t(
                    "res.inotify-value",
                    &[("n", &inotify.watches), ("max", &max)],
                );
                let target = format!("≤ {} %", targets.inotify_share_pct);
                let _ = writeln!(
                    out,
                    "{}",
                    line(
                        "res.inotify",
                        &value,
                        &verdict(targets.inotify_within(inotify), &target)
                    )
                );
            }
            tiers_text(&mut out, r.observation.as_ref());
            if let Some(n) = engine.discovered {
                let _ = writeln!(out, "{}", t("res.discovered", &[("n", &n)]));
            }
        }
        None => {
            let _ = writeln!(out, "{}", t("res.engine-stopped", &[]));
        }
    }
    disk_text(&mut out, view);
    if let Some(engine) = &view.engine {
        let pools = if engine.result.pools.is_some() {
            t("res.pools-reported", &[])
        } else {
            na()
        };
        let _ = writeln!(out, "{}", t("res.pools", &[("value", &pools)]));
        let power = match engine.result.power_saving {
            Some(p) if p.active => t("res.power-on", &[]),
            Some(_) => t("res.power-off", &[]),
            None => na(),
        };
        let _ = writeln!(out, "{}", t("res.power", &[("value", &power)]));
    }
    out
}

/// The observation tiers (TS-GRP-006, N8): how many repos are active and
/// how many dormant, and what each tier keeps watched. Descriptors are the
/// process's, shown once above: they cannot be split by tier.
fn tiers_text(out: &mut String, observation: Option<&ObservationUsage>) {
    let Some(o) = observation else {
        let _ = writeln!(out, "{}", t("res.tiers-na", &[]));
        return;
    };
    let _ = writeln!(
        out,
        "{}",
        t(
            "res.tiers",
            &[
                ("active", &o.active.repos),
                ("dormant", &o.dormant.repos),
                ("waking", &o.waking.repos),
            ],
        )
    );
    let _ = writeln!(
        out,
        "  {}",
        t(
            "res.tier-active",
            &[
                ("worktrees", &o.active.worktrees),
                ("watches", &o.active.watches),
            ],
        )
    );
    let _ = writeln!(
        out,
        "  {}",
        t(
            "res.tier-dormant",
            &[
                ("worktrees", &o.dormant.worktrees),
                ("watches", &o.dormant.watches),
            ],
        )
    );
    let _ = writeln!(
        out,
        "  {}",
        t(
            "res.tier-safety",
            &[
                ("sweep", &window(o.dormant.sweep_interval_s)),
                ("reconcile", &window(o.dormant.reconcile_interval_s)),
                ("cpu", &o.dormant.safety_net_cpu_pct.map_or_else(na, pct)),
            ],
        )
    );
    let _ = writeln!(
        out,
        "  {}",
        t("res.tier-degraded", &[("n", &o.degraded.worktrees)])
    );
}

fn disk_text(out: &mut String, view: &View) {
    let targets = &view.targets;
    let disk = &view.disk;
    let shown = |n: u64| {
        if disk.complete {
            bytes(n)
        } else {
            t("res.at-least", &[("value", &bytes(n))])
        }
    };
    let target = format!("≤ {}", bytes(targets.profile_bytes));
    let _ = writeln!(
        out,
        "{}",
        line(
            "res.profile-disk",
            &shown(disk.profile_bytes),
            &verdict(Some(targets.profile_within(disk.profile_bytes)), &target),
        )
    );
    let _ = writeln!(
        out,
        "{}",
        line(
            "res.tm-disk",
            &shown(disk.time_machine_bytes()),
            &t(
                "res.tm-reference",
                &[("target", &bytes(targets.time_machine_bytes))]
            ),
        )
    );
    for repo in &disk.time_machine {
        let name = Untrusted::new(repo_name(&view.repos, &repo.repo_id)).sanitized();
        let _ = writeln!(
            out,
            "  {}",
            t(
                "res.tm-repo",
                &[("repo", &name), ("value", &shown(repo.bytes))]
            )
        );
    }
}

fn judged(value: Option<u64>, target: Option<u64>, within: Option<bool>) -> Value {
    json!({ "value": value, "target": target, "within": within })
}

/// The JSON output: the same values in fixed units (percent, bytes and
/// counts), each with its target and whether it meets it. No presentation
/// text and nothing from the repo's content.
pub fn json(view: &View) -> Value {
    let targets = &view.targets;
    let disk = &view.disk;
    let engine = view.engine.as_ref().map(|engine| {
        let r = &engine.result;
        let cpu = &r.process.cpu;
        json!({
            "pid": engine.pid,
            "cpu": {
                "mean_pct": cpu.mean_pct,
                "peak_pct": cpu.peak_pct,
                "window_s": cpu.window_s,
                "target_pct": targets.cpu_pct,
                "within": cpu.mean_pct.map(|m| targets.cpu_within(m)),
            },
            "rss_bytes": judged(
                r.process.rss_bytes,
                Some(targets.rss_bytes),
                r.process.rss_bytes.map(|v| targets.rss_within(v)),
            ),
            "open_fds": judged(
                r.process.open_fds,
                targets.open_fds,
                r.process.open_fds.and_then(|v| targets.fds_within(v)),
            ),
            "watches": {
                "roots": r.watches.roots,
                "backend": r.watches.backend.map(|b| b.as_str()),
                "inotify": r.watches.inotify.map(|i| json!({
                    "watches": i.watches,
                    "max_user_watches": i.max_user_watches,
                    "target_share_pct": targets.inotify_share_pct,
                    "within": targets.inotify_within(&i),
                })),
            },
            "pools": r.pools,
            "power_saving": r.power_saving,
            "observation": r.observation,
            "discovered_repos": engine.discovered,
        })
    });
    let repos: Vec<Value> = disk
        .time_machine
        .iter()
        .map(|r| {
            json!({
                "repo_id": r.repo_id,
                "path": view.repos.get(&r.repo_id),
                "bytes": r.bytes,
            })
        })
        .collect();
    json!({
        "running": view.engine.is_some(),
        "engine": engine,
        "disk": {
            "complete": disk.complete,
            "profile_bytes": judged(
                Some(disk.profile_bytes),
                Some(targets.profile_bytes),
                Some(targets.profile_within(disk.profile_bytes)),
            ),
            "time_machine": {
                "bytes": disk.time_machine_bytes(),
                // A reference until US-TMC-022 enforces the cap: not judged.
                "reference_bytes": targets.time_machine_bytes,
                "within": Value::Null,
                "repos": repos,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitraptor_api::resources::{CpuUsage, ProcessUsage, RepoDisk, TARGETS, WatchUsage};

    fn result(rss: u64) -> ResourcesResult {
        ResourcesResult {
            process: ProcessUsage {
                cpu: CpuUsage {
                    mean_pct: Some(0.12),
                    peak_pct: Some(0.4),
                    window_s: 600,
                },
                rss_bytes: Some(rss),
                open_fds: Some(31),
            },
            watches: WatchUsage {
                roots: 3,
                backend: Some(gitraptor_api::resources::WatchBackendKind::Fsevents),
                inotify: None,
            },
            disk: DiskUsage {
                profile_bytes: 2 * 1024 * 1024,
                time_machine: vec![RepoDisk {
                    repo_id: "r1".into(),
                    bytes: 3 * 1024 * 1024,
                }],
                complete: true,
            },
            pools: None,
            power_saving: None,
            observation: None,
            targets: TARGETS,
        }
    }

    fn repos() -> HashMap<String, String> {
        HashMap::from([("r1".to_owned(), "/w/demo/.git".to_owned())])
    }

    #[test]
    fn units_are_binary() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(150 * 1024 * 1024), "150.0 MiB");
        assert_eq!(bytes(10 * 1024 * 1024 * 1024), "10.0 GiB");
    }

    #[test]
    fn json_is_judged_against_the_targets() {
        let v = json(&View::running(7, result(200 * 1024 * 1024), repos()));
        assert_eq!(v["engine"]["rss_bytes"]["within"], false);
        assert_eq!(v["engine"]["cpu"]["within"], true);
        assert_eq!(v["engine"]["open_fds"]["within"], true);
        assert_eq!(
            v["disk"]["time_machine"]["repos"][0]["path"],
            "/w/demo/.git"
        );
        assert!(v["disk"]["time_machine"]["within"].is_null());
    }

    #[test]
    fn an_unknown_repo_is_shown_by_its_id() {
        let mut view = View::running(7, result(1), HashMap::new());
        view.disk.time_machine[0].repo_id = "abc".into();
        assert!(text(&view).contains("abc"));
    }

    /// A daemon without the tiers (or before its observer starts): the
    /// text says "not available" and the JSON carries `null`.
    #[test]
    fn without_tiers_they_are_not_available() {
        let view = View::running(7, result(1), repos());
        assert!(text(&view).contains(&t("res.tiers-na", &[])));
        let v = json(&view);
        let engine = v["engine"].as_object().unwrap();
        assert!(engine.contains_key("observation"));
        assert!(engine["observation"].is_null());
    }

    /// US-GRP-020: the discovered repos are counted and cost nothing.
    #[test]
    fn discovered_repos_are_shown_at_zero_cost() {
        let view = View::running(7, result(1), repos()).with_discovered(Some(4));
        assert!(text(&view).contains(&t("res.discovered", &[("n", &4)])));
        assert_eq!(json(&view)["engine"]["discovered_repos"], 4);
        let older = View::running(7, result(1), repos());
        assert!(json(&older)["engine"]["discovered_repos"].is_null());
    }

    /// Every key this view uses exists in both catalogs.
    #[test]
    fn every_key_has_its_messages() {
        for key in [
            "res.engine-running",
            "res.engine-stopped",
            "res.line",
            "res.cpu",
            "res.cpu-value",
            "res.window-min",
            "res.window-s",
            "res.rss",
            "res.fds",
            "res.watches",
            "res.roots",
            "res.inotify",
            "res.inotify-value",
            "res.profile-disk",
            "res.tm-disk",
            "res.tm-reference",
            "res.tm-repo",
            "res.at-least",
            "res.within",
            "res.over",
            "res.not-judged",
            "res.no-target",
            "res.not-available",
            "res.pools",
            "res.pools-reported",
            "res.power",
            "res.power-on",
            "res.power-off",
            "res.restart-engine",
            "res.tiers",
            "res.tiers-na",
            "res.tier-active",
            "res.tier-dormant",
            "res.tier-safety",
            "res.tier-degraded",
            "res.discovered",
        ] {
            assert!(crate::i18n::has_key(key), "{key}");
        }
    }
}
