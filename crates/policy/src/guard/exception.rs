//! Exception to a denied operation (US-GRD-006): parse the git argv, resolve it against the repo
//! facts and decide which rules an exception may skip. Pure: no I/O, no clock, no randomness.

use std::collections::BTreeMap;

use gitraptor_api::guard::{ExceptionRefusal, LoggedOperation, Reason, Rule};

/// Why an argv is not an operation an exception can cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    Form,
    Operation,
}

impl Unsupported {
    /// The refusal this maps to.
    pub fn refusal(self) -> ExceptionRefusal {
        todo!()
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

/// Parses a git argv into a covered form.
pub fn parse(argv: &[String]) -> Result<Request, Unsupported> {
    let _ = argv;
    todo!()
}

/// Resolves `req` against `facts` into its binding.
pub fn resolve(req: &Request, facts: &RepoFacts) -> Result<Binding, Unsupported> {
    let _ = (req, facts);
    todo!()
}

/// Whether an exception may skip `rule`.
pub fn exceptable(rule: Rule) -> bool {
    let _ = rule;
    todo!()
}

/// The rules an exception announces for `reasons`, in order of appearance without duplicates.
pub fn announce(reasons: &[Reason], wants_prior: bool) -> Result<Vec<Rule>, ExceptionRefusal> {
    let _ = (reasons, wants_prior);
    todo!()
}

/// Whether `announced` covers every one of `reasons`.
pub fn covers(announced: &[Rule], reasons: &[Rule]) -> bool {
    let _ = (announced, reasons);
    todo!()
}
