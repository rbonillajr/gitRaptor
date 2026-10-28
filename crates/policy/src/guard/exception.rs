//! Exception to a denied operation (US-GRD-006): parse the git argv, resolve it against the repo
//! facts and decide which rules an exception may skip. Pure: no I/O, no clock, no randomness.

use std::collections::BTreeMap;

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    ExceptionRefusal, LoggedOperation, LoggedRef, MAX_EXCEPTION_ARG_BYTES, MAX_EXCEPTION_ARGV,
    Reason, RefChange, Rule,
};

use super::refs::{branch_ref, short};

/// Why an argv is not an operation an exception can cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    Form,
    Operation,
}

impl Unsupported {
    /// The refusal this maps to.
    pub fn refusal(self) -> ExceptionRefusal {
        match self {
            Self::Form => ExceptionRefusal::UnsupportedForm,
            Self::Operation => ExceptionRefusal::UnsupportedOperation,
        }
    }
}

/// A parsed git invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The `-C` directories, in order.
    pub dirs: Vec<String>,
    pub form: Form,
}

/// The forms an exception can cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    Push {
        remote: Option<String>,
        refspecs: Vec<Refspec>,
        force: bool,
    },
    BranchDelete {
        names: Vec<String>,
        force: bool,
    },
    Rebase {
        onto: Option<String>,
        upstream: Option<String>,
        branch: Option<String>,
    },
}

/// One push refspec: `src: None` deletes, `dst: None` means the same name as `src`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refspec {
    pub src: Option<String>,
    pub dst: Option<String>,
    pub force: bool,
}

/// The repo state `resolve` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFacts {
    /// Short branch of HEAD; `None` when detached.
    pub head: Option<String>,
    pub push_default: Option<String>,
    pub remotes: Vec<String>,
    pub push_remote: Option<String>,
    pub upstreams: BTreeMap<String, Upstream>,
    /// `(remote, refs/heads/x)` to the oid of the tracking copy.
    pub tracking: BTreeMap<(String, String), String>,
    /// Name as given to its oid.
    pub revs: BTreeMap<String, String>,
}

/// The upstream of a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub remote: String,
    pub merge: String,
}

/// One ref movement an exception is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    Ref {
        remote: Option<String>,
        refname: String,
        old: Option<String>,
        new: Option<String>,
    },
    Rebase {
        refname: String,
        base: String,
    },
}

/// The operation and the transitions an exception covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub operation: LoggedOperation,
    pub transitions: Vec<Transition>,
}

/// Parses a git argv into a covered form: `git [-C <dir>]… push|branch|rebase …`.
///
/// # Errors
///
/// [`Unsupported::Operation`] for any other subcommand (aliases included) and for `branch`
/// without a delete option; [`Unsupported::Form`] for anything else outside the admitted forms:
/// an argv over its bounds, a program other than literally `git`, a global option other than
/// `-C`, an option outside the closed lists, a wildcard, a URL remote or `pull`.
pub fn parse(argv: &[String]) -> Result<Request, Unsupported> {
    if argv.len() > MAX_EXCEPTION_ARGV || argv.iter().any(|a| a.len() > MAX_EXCEPTION_ARG_BYTES) {
        return Err(Unsupported::Form);
    }
    let Some((program, rest)) = argv.split_first() else {
        return Err(Unsupported::Form);
    };
    if program != "git" {
        return Err(Unsupported::Form);
    }
    let mut dirs = Vec::new();
    let mut args = rest.iter();
    let sub = loop {
        let Some(arg) = args.next() else {
            return Err(Unsupported::Form);
        };
        if arg == "-C" {
            dirs.push(args.next().ok_or(Unsupported::Form)?.clone());
            continue;
        }
        // Every other global option (`-c`, `--git-dir`, `--work-tree`, ...) could change the repo or
        // its configuration after the normalization; one Git does not document is no better.
        if arg.starts_with('-') {
            return Err(Unsupported::Form);
        }
        break arg.as_str();
    };
    let args: Vec<&str> = args.map(String::as_str).collect();
    let form = match sub {
        "push" => parse_push(&args)?,
        "branch" => parse_branch(&args)?,
        "rebase" => parse_rebase(&args)?,
        // Its fetch runs git grandchildren that do not inherit the binding.
        "pull" => return Err(Unsupported::Form),
        _ => return Err(Unsupported::Operation),
    };
    Ok(Request { dirs, form })
}

/// A name the exception can bind: not empty, not an option, no wildcard.
fn name(arg: &str) -> Result<String, Unsupported> {
    if arg.is_empty() || arg.starts_with('-') || arg.contains(['*', '?', '[']) {
        return Err(Unsupported::Form);
    }
    Ok(arg.to_owned())
}

/// A remote given by name: a URL or a path (`://`, scp `host:path`, `/`, `.`, `~`) is refused.
fn remote_name(arg: &str) -> Result<String, Unsupported> {
    if arg.contains(':') || arg.contains('\\') || arg.starts_with(['/', '.', '~']) {
        return Err(Unsupported::Form);
    }
    name(arg)
}

fn parse_push(args: &[&str]) -> Result<Form, Unsupported> {
    let (mut force, mut delete) = (false, false);
    let mut positional = Vec::new();
    for &arg in args {
        match arg {
            "-f" | "--force" | "--force-with-lease" => force = true,
            _ if arg.starts_with("--force-with-lease=") => force = true,
            // Only what git prints changes: the refs pushed are the same.
            "--force-if-includes" | "-q" | "--quiet" => {}
            "-d" | "--delete" => delete = true,
            _ if arg.starts_with('-') => return Err(Unsupported::Form),
            _ => positional.push(arg),
        }
    }
    let Some((remote, specs)) = positional.split_first() else {
        return if delete {
            Err(Unsupported::Form)
        } else {
            Ok(Form::Push {
                remote: None,
                refspecs: Vec::new(),
                force,
            })
        };
    };
    let remote = remote_name(remote)?;
    let refspecs = if delete {
        if specs.is_empty() {
            return Err(Unsupported::Form);
        }
        specs
            .iter()
            .map(|dst| {
                Ok(Refspec {
                    src: None,
                    dst: Some(name(dst)?),
                    force: false,
                })
            })
            .collect::<Result<_, _>>()?
    } else {
        specs.iter().map(|s| refspec(s)).collect::<Result<_, _>>()?
    };
    Ok(Form::Push {
        remote: Some(remote),
        refspecs,
        force,
    })
}

/// `[+]src`, `[+]src:dst` or `:dst`; `:` alone (matching) and `src:` are refused.
fn refspec(text: &str) -> Result<Refspec, Unsupported> {
    let (force, text) = match text.strip_prefix('+') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    match text.split_once(':') {
        None => Ok(Refspec {
            src: Some(name(text)?),
            dst: None,
            force,
        }),
        Some(("", dst)) if !force => Ok(Refspec {
            src: None,
            dst: Some(name(dst)?),
            force,
        }),
        Some((src, dst)) if !src.is_empty() && !dst.contains(':') => Ok(Refspec {
            src: Some(name(src)?),
            dst: Some(name(dst)?),
            force,
        }),
        Some(_) => Err(Unsupported::Form),
    }
}

fn parse_branch(args: &[&str]) -> Result<Form, Unsupported> {
    let (mut delete, mut force, mut other) = (false, false, false);
    let mut names = Vec::new();
    for &arg in args {
        match arg {
            "-d" | "--delete" => delete = true,
            "-D" => (delete, force) = (true, true),
            "-f" | "--force" => force = true,
            "-q" | "--quiet" => {}
            _ if arg.starts_with('-') => other = true,
            _ => names.push(arg),
        }
    }
    if !delete {
        // Creating, renaming or listing branches is not an operation the exception covers.
        return Err(Unsupported::Operation);
    }
    if other || names.is_empty() {
        return Err(Unsupported::Form);
    }
    let names = names.into_iter().map(name).collect::<Result<_, _>>()?;
    Ok(Form::BranchDelete { names, force })
}

fn parse_rebase(args: &[&str]) -> Result<Form, Unsupported> {
    let mut onto = None;
    let mut positional = Vec::new();
    let mut args = args.iter();
    while let Some(&arg) = args.next() {
        if arg == "--onto" {
            onto = Some(name(args.next().ok_or(Unsupported::Form)?)?);
        } else if let Some(base) = arg.strip_prefix("--onto=") {
            onto = Some(name(base)?);
        } else if arg == "-q" || arg == "--quiet" {
            // Only what git prints changes.
        } else if arg.starts_with('-') {
            // `-i`, `--exec`, `--root` and any other option: interactive or running commands.
            return Err(Unsupported::Form);
        } else {
            positional.push(name(arg)?);
        }
    }
    if positional.len() > 2 {
        return Err(Unsupported::Form);
    }
    let mut positional = positional.into_iter();
    Ok(Form::Rebase {
        onto,
        upstream: positional.next(),
        branch: positional.next(),
    })
}

/// Resolves `req` against `facts` into its binding: the normalized operation (names only, never
/// argv or oids) and the concrete transitions the token is bound to.
///
/// # Errors
///
/// [`Unsupported::Form`] when the request does not resolve to concrete transitions: an unknown
/// remote or rev, a detached HEAD where the branch is implied, a `push.default` other than
/// `simple`, `current` or `upstream`, or no upstream where one is needed.
pub fn resolve(req: &Request, facts: &RepoFacts) -> Result<Binding, Unsupported> {
    match &req.form {
        Form::Push {
            remote, refspecs, ..
        } => resolve_push(remote.as_deref(), refspecs, facts),
        Form::BranchDelete { names, .. } => resolve_branch_delete(names, facts),
        Form::Rebase {
            onto,
            upstream,
            branch,
        } => resolve_rebase(
            onto.as_deref(),
            upstream.as_deref(),
            branch.as_deref(),
            facts,
        ),
    }
}

fn rev<'a>(facts: &'a RepoFacts, name: &str) -> Result<&'a String, Unsupported> {
    facts.revs.get(name).ok_or(Unsupported::Form)
}

fn head(facts: &RepoFacts) -> Result<&str, Unsupported> {
    facts.head.as_deref().ok_or(Unsupported::Form)
}

/// The full destination ref of `name`: a full ref as given, `HEAD` as the current branch.
fn full_ref(name: &str, facts: &RepoFacts) -> Result<String, Unsupported> {
    if name == "HEAD" {
        return Ok(branch_ref(head(facts)?));
    }
    Ok(if name.starts_with("refs/") {
        name.to_owned()
    } else {
        branch_ref(name)
    })
}

fn resolve_push(
    remote: Option<&str>,
    refspecs: &[Refspec],
    facts: &RepoFacts,
) -> Result<Binding, Unsupported> {
    let upstream = facts.head.as_ref().and_then(|h| facts.upstreams.get(h));
    let remote = remote
        .or(facts.push_remote.as_deref())
        .or(upstream.map(|u| u.remote.as_str()))
        .ok_or(Unsupported::Form)?;
    if !facts.remotes.iter().any(|r| r == remote) {
        return Err(Unsupported::Form);
    }
    let implied;
    let refspecs = if refspecs.is_empty() {
        implied = [default_refspec(remote, facts)?];
        &implied[..]
    } else {
        refspecs
    };
    let mut transitions = Vec::with_capacity(refspecs.len());
    let mut refs = Vec::with_capacity(refspecs.len());
    for spec in refspecs {
        let target = spec
            .dst
            .as_deref()
            .or(spec.src.as_deref())
            .ok_or(Unsupported::Form)?;
        let refname = full_ref(target, facts)?;
        let old = facts
            .tracking
            .get(&(remote.to_owned(), refname.clone()))
            .cloned();
        let new = spec
            .src
            .as_deref()
            .map(|s| rev(facts, s).cloned())
            .transpose()?;
        let change = match (&old, &new) {
            // Deleting a ref with no known copy binds no concrete transition.
            (None, None) => return Err(Unsupported::Form),
            (_, None) => RefChange::Delete,
            // `+` names a forced ref; the global force option covers every ref and so is not
            // read as a change of each.
            _ if spec.force => RefChange::Force,
            (None, Some(_)) => RefChange::Create,
            (Some(_), Some(_)) => RefChange::Update,
        };
        refs.push(LoggedRef {
            name: Untrusted::new(short(&refname)),
            change,
        });
        transitions.push(Transition::Ref {
            remote: Some(remote.to_owned()),
            refname,
            old,
            new,
        });
    }
    Ok(Binding {
        operation: LoggedOperation::Push {
            remote: Some(Untrusted::new(remote)),
            refs,
        },
        transitions,
    })
}

/// What a push without refspec pushes, by `push.default` (unset is `simple`).
fn default_refspec(remote: &str, facts: &RepoFacts) -> Result<Refspec, Unsupported> {
    let branch = head(facts)?;
    let upstream = facts.upstreams.get(branch).filter(|u| u.remote == remote);
    let dst = match facts.push_default.as_deref().unwrap_or("simple") {
        "current" => branch_ref(branch),
        // Git's `simple` pushes as `current` in a triangular workflow: a push to another remote
        // than the one the branch fetches from (its upstream's, or `origin`).
        "simple" if triangular(branch, remote, facts) => branch_ref(branch),
        "simple" => {
            let merge = &upstream.ok_or(Unsupported::Form)?.merge;
            if *merge != branch_ref(branch) {
                return Err(Unsupported::Form);
            }
            merge.clone()
        }
        "upstream" => upstream.ok_or(Unsupported::Form)?.merge.clone(),
        _ => return Err(Unsupported::Form),
    };
    Ok(Refspec {
        src: Some(branch.to_owned()),
        dst: Some(dst),
        force: false,
    })
}

/// Whether pushing `branch` to `remote` is triangular: the branch fetches from another remote.
fn triangular(branch: &str, remote: &str, facts: &RepoFacts) -> bool {
    let fetch = facts
        .upstreams
        .get(branch)
        .map(|u| u.remote.as_str())
        .or_else(|| {
            facts
                .remotes
                .iter()
                .any(|r| r == "origin")
                .then_some("origin")
        });
    fetch.is_some_and(|fetch| fetch != remote)
}

fn resolve_branch_delete(names: &[String], facts: &RepoFacts) -> Result<Binding, Unsupported> {
    let mut transitions = Vec::with_capacity(names.len());
    let mut refs = Vec::with_capacity(names.len());
    for branch in names {
        let old = rev(facts, branch)?.clone();
        refs.push(LoggedRef {
            name: Untrusted::new(branch.as_str()),
            change: RefChange::Delete,
        });
        transitions.push(Transition::Ref {
            remote: None,
            refname: branch_ref(branch),
            old: Some(old),
            new: None,
        });
    }
    Ok(Binding {
        operation: LoggedOperation::RefTransaction { refs },
        transitions,
    })
}

fn resolve_rebase(
    onto: Option<&str>,
    upstream: Option<&str>,
    branch: Option<&str>,
    facts: &RepoFacts,
) -> Result<Binding, Unsupported> {
    let target = match branch {
        Some(given) => {
            rev(facts, given)?;
            given
        }
        None => head(facts)?,
    };
    let upstream_oid = match upstream {
        Some(given) => rev(facts, given)?.clone(),
        None => configured_upstream(target, facts)?,
    };
    let base = match onto {
        Some(onto) => rev(facts, onto)?.clone(),
        None => upstream_oid,
    };
    Ok(Binding {
        operation: LoggedOperation::Rebase {
            upstream: upstream.map(Untrusted::new),
            branch: branch.map(Untrusted::new),
        },
        transitions: vec![Transition::Rebase {
            refname: branch_ref(target),
            base,
        }],
    })
}

/// The oid of the configured upstream of `branch`: its tracking copy, or the local branch when
/// the upstream remote is `.`.
fn configured_upstream(branch: &str, facts: &RepoFacts) -> Result<String, Unsupported> {
    let up = facts.upstreams.get(branch).ok_or(Unsupported::Form)?;
    let oid = if up.remote == "." {
        facts.revs.get(short(&up.merge))
    } else {
        facts.tracking.get(&(up.remote.clone(), up.merge.clone()))
    };
    oid.cloned().ok_or(Unsupported::Form)
}

/// Whether an exception may skip `rule`: the rules of the policy, never an integrity signal
/// (`system.*` but the failed snapshot) nor a rule that only applies to agents.
pub fn exceptable(rule: Rule) -> bool {
    match rule {
        Rule::MinimumForcePush
        | Rule::MinimumBaseBranchDelete
        | Rule::ProtectedBranch
        | Rule::ForbiddenPath
        | Rule::DiffSize
        | Rule::CommitFormat
        | Rule::SnapshotFailed
        | Rule::PermissionCommit
        | Rule::PermissionPush
        | Rule::PermissionForcePush
        | Rule::PermissionResetHard
        | Rule::PermissionBranchDelete
        | Rule::PermissionRebase
        | Rule::PermissionMerge
        | Rule::PermissionWorktreeAdd
        | Rule::PermissionWorktreeRemove => true,
        Rule::Degraded
        | Rule::ChannelNotAuthentic
        | Rule::RepoMismatch
        | Rule::InputRejected
        | Rule::InternalError
        | Rule::AuthorshipTrailerRequired
        | Rule::AuthorshipHumanAuthor
        | Rule::ConfigProtected
        | Rule::RelaxIgnored => false,
    }
}

/// The rules an exception announces for `reasons`, in order of appearance without duplicates,
/// with `system.snapshot-failed` at the end when the operation asks for a prior snapshot.
///
/// # Errors
///
/// [`ExceptionRefusal::NotExceptable`] when a reason cannot be skipped;
/// [`ExceptionRefusal::NotDenied`] when nothing would be skipped.
pub fn announce(reasons: &[Reason], wants_prior: bool) -> Result<Vec<Rule>, ExceptionRefusal> {
    let mut rules: Vec<Rule> = Vec::new();
    let prior = wants_prior.then_some(Rule::SnapshotFailed);
    for rule in reasons.iter().map(|r| r.rule).chain(prior) {
        if !exceptable(rule) {
            return Err(ExceptionRefusal::NotExceptable);
        }
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    if rules.is_empty() {
        return Err(ExceptionRefusal::NotDenied);
    }
    Ok(rules)
}

/// Whether `announced` covers every one of `reasons`: each is skippable and was announced.
pub fn covers(announced: &[Rule], reasons: &[Rule]) -> bool {
    reasons
        .iter()
        .all(|rule| exceptable(*rule) && announced.contains(rule))
}
