//! Owner and DACL checks of Windows files and folders (SEC-10, SEC-06; TD-GRP-001).
//!
//! The DACL is copied out of Windows as bytes and parsed here in safe code, so the parser and
//! the rules build and are tested on every OS. Only the functions that touch the file system
//! are Windows-only. Anything that cannot be read or understood is rejected (fail-closed).

use std::fmt;

/// `FILE_WRITE_DATA` on a file, `FILE_ADD_FILE` on a folder.
const FILE_WRITE_DATA: u32 = 0x0002;
/// `FILE_APPEND_DATA` on a file, `FILE_ADD_SUBDIRECTORY` on a folder.
const FILE_APPEND_DATA: u32 = 0x0004;
const FILE_WRITE_EA: u32 = 0x0010;
const FILE_DELETE_CHILD: u32 = 0x0040;
const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
const DELETE: u32 = 0x0001_0000;
const WRITE_DAC: u32 = 0x0004_0000;
const WRITE_OWNER: u32 = 0x0008_0000;
const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
const GENERIC_ALL: u32 = 0x1000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

/// Rights that replace, rename or re-permission the object itself.
const TAKEOVER: u32 = DELETE | WRITE_DAC | WRITE_OWNER | MAXIMUM_ALLOWED | GENERIC_ALL;
/// Write rights on the executable.
const EXECUTABLE_WRITE: u32 = TAKEOVER
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_WRITE_ATTRIBUTES
    | GENERIC_WRITE;
/// Write rights on the folder of the executable: no new file next to it (DLL planting).
const EXECUTABLE_DIR_WRITE: u32 = TAKEOVER
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_DELETE_CHILD
    | FILE_WRITE_ATTRIBUTES
    | GENERIC_WRITE;
/// Write rights on a folder above: renaming or deleting a child replaces the path. Adding a
/// new subfolder does not (`C:\` grants it to Authenticated Users).
const ANCESTOR_WRITE: u32 = TAKEOVER | FILE_DELETE_CHILD | GENERIC_WRITE;

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0x00;
const ACCESS_DENIED_ACE_TYPE: u8 = 0x01;
const ACCESS_DENIED_OBJECT_ACE_TYPE: u8 = 0x06;
const ACCESS_DENIED_CALLBACK_ACE_TYPE: u8 = 0x0A;
const ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE: u8 = 0x0C;
const INHERIT_ONLY_ACE: u8 = 0x08;

/// A security identifier, kept as its binary form and compared byte by byte (what `EqualSid`
/// does on two valid SIDs).
#[derive(Clone, PartialEq, Eq)]
pub struct Sid(Vec<u8>);

impl Sid {
    /// Parses a binary SID: revision 1, at most 15 sub-authorities, and exactly its length.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let count = *bytes.get(1)? as usize;
        (bytes.first() == Some(&1) && count <= 15 && bytes.len() == 8 + 4 * count)
            .then(|| Self(bytes.to_vec()))
    }

    fn from_parts(authority: u8, subs: &[u32]) -> Self {
        let mut bytes = vec![1, subs.len() as u8, 0, 0, 0, 0, 0, authority];
        for sub in subs {
            bytes.extend_from_slice(&sub.to_le_bytes());
        }
        Self(bytes)
    }

    /// `NT AUTHORITY\SYSTEM` (S-1-5-18).
    pub fn local_system() -> Self {
        Self::from_parts(5, &[18])
    }

    /// `BUILTIN\Administrators` (S-1-5-32-544).
    pub fn administrators() -> Self {
        Self::from_parts(5, &[32, 544])
    }

    /// `NT SERVICE\TrustedInstaller`.
    pub fn trusted_installer() -> Self {
        Self::from_parts(
            5,
            &[
                80,
                956_008_885,
                3_418_522_649,
                1_831_038_044,
                1_853_292_631,
                2_271_478_464,
            ],
        )
    }

    /// `OWNER RIGHTS` (S-1-3-4): stands for whoever owns the object.
    pub fn owner_rights() -> Self {
        Self::from_parts(3, &[4])
    }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let authority = self.0[2..8]
            .iter()
            .fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
        write!(f, "S-{}-{authority}", self.0[0])?;
        for sub in self.0[8..].as_chunks::<4>().0 {
            write!(f, "-{}", u32::from_le_bytes(*sub))?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sid({self})")
    }
}

/// Kind of an access control entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AceKind {
    /// `ACCESS_ALLOWED_ACE_TYPE`, the only one evaluated.
    Allow,
    /// Any deny type: it can only remove access, so it is ignored.
    Deny,
    /// Any other type (callback, object, compound): rejected.
    Other(u8),
}

/// One entry of a DACL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ace {
    pub kind: AceKind,
    /// Applies only to children created later, not to the object itself.
    pub inherit_only: bool,
    pub mask: u32,
    /// Trustee of an [`AceKind::Allow`] entry; `None` for the other kinds.
    pub sid: Option<Sid>,
}

/// Why an object did not pass.
#[derive(Debug)]
pub enum AclError {
    /// Owner or DACL could not be read.
    Unreadable(std::io::Error),
    /// Not on a local volume with persistent ACLs (a network share, FAT).
    NotLocal,
    /// A component of the path is a reparse point (symlink, junction, mount point).
    ReparsePoint,
    /// Missing or NULL DACL: full access for everyone.
    NullDacl,
    /// The DACL could not be parsed.
    Malformed,
    /// The DACL has an entry of a kind that is not understood.
    UnknownAce(u8),
    /// The owner is not trusted.
    UntrustedOwner(Sid),
    /// An untrusted SID may write, rename or re-permission it.
    UntrustedWriter(Sid),
    /// An untrusted SID has some access to a private folder.
    UntrustedAccess(Sid),
}

impl fmt::Display for AclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(e) => write!(f, "ACL not readable: {e}"),
            Self::NotLocal => write!(f, "not on a local volume with persistent ACLs"),
            Self::ReparsePoint => write!(f, "is or goes through a reparse point"),
            Self::NullDacl => write!(f, "has no DACL (full access for everyone)"),
            Self::Malformed => write!(f, "malformed DACL"),
            Self::UnknownAce(t) => write!(f, "DACL entry of unknown type {t:#04x}"),
            Self::UntrustedOwner(s) => write!(f, "owned by untrusted {s}"),
            Self::UntrustedWriter(s) => write!(f, "writable by untrusted {s}"),
            Self::UntrustedAccess(s) => write!(f, "accessible to untrusted {s}"),
        }
    }
}

impl std::error::Error for AclError {}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Parses the binary form of an ACL, checking every size against the buffer.
pub fn parse_acl(bytes: &[u8]) -> Result<Vec<Ace>, AclError> {
    let malformed = || AclError::Malformed;
    let revision = *bytes.first().ok_or_else(malformed)?;
    if !(2..=4).contains(&revision) {
        return Err(malformed());
    }
    let size = usize::from(u16_at(bytes, 2).ok_or_else(malformed)?);
    let count = u16_at(bytes, 4).ok_or_else(malformed)?;
    let acl = bytes.get(..size).ok_or_else(malformed)?;
    let mut offset = 8;
    let mut aces = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let ace_type = *acl.get(offset).ok_or_else(malformed)?;
        let flags = *acl.get(offset + 1).ok_or_else(malformed)?;
        let ace_size = usize::from(u16_at(acl, offset + 2).ok_or_else(malformed)?);
        if ace_size < 8 {
            return Err(malformed());
        }
        let ace = acl.get(offset..offset + ace_size).ok_or_else(malformed)?;
        let mask = u32_at(ace, 4).ok_or_else(malformed)?;
        let kind = match ace_type {
            ACCESS_ALLOWED_ACE_TYPE => AceKind::Allow,
            ACCESS_DENIED_ACE_TYPE
            | ACCESS_DENIED_OBJECT_ACE_TYPE
            | ACCESS_DENIED_CALLBACK_ACE_TYPE
            | ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE => AceKind::Deny,
            other => AceKind::Other(other),
        };
        let sid = if kind == AceKind::Allow {
            let sid = ace.get(8..).ok_or_else(malformed)?;
            let len = 8 + 4 * usize::from(*sid.get(1).ok_or_else(malformed)?);
            Some(Sid::from_bytes(sid.get(..len).ok_or_else(malformed)?).ok_or_else(malformed)?)
        } else {
            None
        };
        aces.push(Ace {
            kind,
            inherit_only: flags & INHERIT_ONLY_ACE != 0,
            mask,
            sid,
        });
        offset += ace_size;
    }
    Ok(aces)
}

/// What Windows says about one object, already copied out of it.
#[derive(Debug, Clone)]
pub struct Security {
    pub owner: Sid,
    /// `None` for a missing or NULL DACL.
    pub dacl: Option<Vec<Ace>>,
    pub reparse_point: bool,
    pub persistent_acls: bool,
}

/// The place of an object in a check, which sets the rights others must not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The executable itself.
    Executable,
    /// The folder that holds the executable.
    ExecutableDir,
    /// A folder above it, up to the root of the volume.
    Ancestor,
    /// A folder of the profile: only the user, SYSTEM and Administrators get any access.
    PrivateDir,
}

/// Applies the rules of `role` to `security`, with `user` as the current user.
pub fn evaluate(security: &Security, role: Role, user: &Sid) -> Result<(), AclError> {
    if security.reparse_point {
        return Err(AclError::ReparsePoint);
    }
    if !security.persistent_acls {
        return Err(AclError::NotLocal);
    }
    let mut trusted = vec![user.clone(), Sid::local_system(), Sid::administrators()];
    if role != Role::PrivateDir {
        trusted.push(Sid::trusted_installer());
    }
    if !trusted.contains(&security.owner) {
        return Err(AclError::UntrustedOwner(security.owner.clone()));
    }
    // The owner is trusted, so `OWNER RIGHTS` stands for a trusted SID.
    trusted.push(Sid::owner_rights());
    let (forbidden, error): (u32, fn(Sid) -> AclError) = match role {
        Role::Executable => (EXECUTABLE_WRITE, AclError::UntrustedWriter),
        Role::ExecutableDir => (EXECUTABLE_DIR_WRITE, AclError::UntrustedWriter),
        Role::Ancestor => (ANCESTOR_WRITE, AclError::UntrustedWriter),
        Role::PrivateDir => (u32::MAX, AclError::UntrustedAccess),
    };
    let dacl = security.dacl.as_ref().ok_or(AclError::NullDacl)?;
    for ace in dacl {
        // An inherit-only entry does not apply to the object. On the executable chain a new
        // child needs a right that is checked anyway; in a private folder it would reach every
        // file created later, so it counts there.
        if ace.inherit_only && role != Role::PrivateDir {
            continue;
        }
        match (&ace.kind, &ace.sid) {
            (AceKind::Deny, _) => {}
            (AceKind::Other(t), _) => return Err(AclError::UnknownAce(*t)),
            (AceKind::Allow, Some(sid)) => {
                if ace.mask & forbidden != 0 && !trusted.contains(sid) {
                    return Err(error(sid.clone()));
                }
            }
            (AceKind::Allow, None) => return Err(AclError::Malformed),
        }
    }
    Ok(())
}

#[cfg(windows)]
pub use os::*;

#[cfg(windows)]
mod os {
    use std::path::{Component, Path, Prefix};

    use super::{AclError, Role, Security, Sid, evaluate, parse_acl};
    use crate::ffi_acl;

    /// The user the process runs as.
    pub fn current_user_sid() -> std::io::Result<Sid> {
        let bytes = ffi_acl::current_user_sid()?;
        Sid::from_bytes(&bytes).ok_or_else(|| std::io::Error::other("invalid user SID"))
    }

    fn read(path: &Path) -> Result<Security, AclError> {
        let raw = ffi_acl::read_security(path).map_err(AclError::Unreadable)?;
        Ok(Security {
            owner: Sid::from_bytes(&raw.owner).ok_or(AclError::Malformed)?,
            dacl: raw.dacl.as_deref().map(parse_acl).transpose()?,
            reparse_point: raw.reparse_point,
            persistent_acls: raw.persistent_acls,
        })
    }

    fn check(path: &Path, role: Role, user: &Sid) -> Result<(), AclError> {
        evaluate(&read(path)?, role, user)
    }

    /// Only drive-letter paths: a network share is judged by another machine's groups.
    fn is_local_disk(path: &Path) -> bool {
        matches!(
            path.components().next(),
            Some(Component::Prefix(p))
                if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        )
    }

    /// Checks that only the user, SYSTEM, Administrators or TrustedInstaller own or can
    /// replace the executable at the canonical `path` (SEC-10): the file, its folder and every
    /// folder above it. For the Git for Windows launcher (`<root>\cmd\git.exe`) the Git it
    /// starts (`<root>\<mingw64|ucrt64|clangarm64|mingw32>\bin\git.exe`, `<root>\bin\git.exe`)
    /// is checked as well when present.
    pub fn verify_trusted_executable(path: &Path) -> Result<(), AclError> {
        if !is_local_disk(path) {
            return Err(AclError::NotLocal);
        }
        let user = current_user_sid().map_err(AclError::Unreadable)?;
        check_chain(path, &user)?;
        let launcher_root = path
            .parent()
            .filter(|dir| {
                dir.file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case("cmd"))
            })
            .and_then(Path::parent);
        if let Some(root) = launcher_root {
            for sub in ["mingw64", "ucrt64", "clangarm64", "mingw32"] {
                let target = root.join(sub).join("bin").join("git.exe");
                if std::fs::symlink_metadata(&target).is_ok() {
                    check(&target, Role::Executable, &user)?;
                    check(&root.join(sub).join("bin"), Role::ExecutableDir, &user)?;
                    check(&root.join(sub), Role::Ancestor, &user)?;
                }
            }
            let bin_git = root.join("bin").join("git.exe");
            if std::fs::symlink_metadata(&bin_git).is_ok() {
                check(&bin_git, Role::Executable, &user)?;
                check(&root.join("bin"), Role::ExecutableDir, &user)?;
            }
        }
        Ok(())
    }

    fn check_chain(path: &Path, user: &Sid) -> Result<(), AclError> {
        check(path, Role::Executable, user)?;
        let dir = path.parent().ok_or(AclError::Malformed)?;
        check(dir, Role::ExecutableDir, user)?;
        for ancestor in dir.ancestors().skip(1) {
            check(ancestor, Role::Ancestor, user)?;
        }
        Ok(())
    }

    /// Checks that a profile folder is owned by and accessible only to the user, SYSTEM and
    /// Administrators, counting the entries its children would inherit (SEC-06).
    pub fn verify_private_dir(path: &Path) -> Result<(), AclError> {
        let user = current_user_sid().map_err(AclError::Unreadable)?;
        check(path, Role::PrivateDir, &user)
    }

    /// Creates one folder owned by the user with a protected DACL (no inheritance): full
    /// control for the user, SYSTEM and Administrators, inherited by everything created inside.
    pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
        let user = current_user_sid()?;
        let sddl = format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
        ffi_acl::create_dir_with_sddl(path, &sddl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> Sid {
        Sid::from_parts(5, &[21, 1, 2, 3, 1001])
    }

    fn users() -> Sid {
        Sid::from_parts(5, &[32, 545])
    }

    fn everyone() -> Sid {
        Sid::from_parts(1, &[0])
    }

    /// Builds the binary form of an ACL.
    fn acl(aces: &[(u8, u8, u32, &Sid)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (ace_type, flags, mask, sid) in aces {
            let size = 8 + sid.0.len();
            body.extend_from_slice(&[*ace_type, *flags]);
            body.extend_from_slice(&(size as u16).to_le_bytes());
            body.extend_from_slice(&mask.to_le_bytes());
            body.extend_from_slice(&sid.0);
        }
        let mut bytes = vec![2, 0];
        bytes.extend_from_slice(&((8 + body.len()) as u16).to_le_bytes());
        bytes.extend_from_slice(&(aces.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend(body);
        bytes
    }

    fn security(owner: Sid, aces: &[(u8, u8, u32, &Sid)]) -> Security {
        Security {
            owner,
            dacl: Some(parse_acl(&acl(aces)).unwrap()),
            reparse_point: false,
            persistent_acls: true,
        }
    }

    const RX: u32 = 0x0012_00A9;
    const FULL: u32 = 0x001F_01FF;
    const MODIFY: u32 = 0x0013_01BF;

    #[test]
    fn sid_strings() {
        assert_eq!(Sid::local_system().to_string(), "S-1-5-18");
        assert_eq!(Sid::administrators().to_string(), "S-1-5-32-544");
        assert_eq!(
            Sid::trusted_installer().to_string(),
            "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
        );
        assert_eq!(Sid::from_bytes(&[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0]), None);
        assert_eq!(Sid::from_bytes(&[2, 0, 0, 0, 0, 0, 0, 5]), None);
    }

    /// The ACL measured on `C:\Program Files\Git\cmd\git.exe` of the real Windows.
    #[test]
    fn program_files_git_passes() {
        let (system, admins) = (Sid::local_system(), Sid::administrators());
        let file = security(
            admins.clone(),
            &[
                (0, 0x10, FULL, &system),
                (0, 0x10, FULL, &admins),
                (0, 0x10, RX, &users()),
            ],
        );
        assert!(evaluate(&file, Role::Executable, &user()).is_ok());
        // `C:\`: Authenticated Users may add subfolders and get inheritable Modify.
        let auth = Sid::from_parts(5, &[11]);
        let root = security(
            Sid::trusted_installer(),
            &[
                (0, 0x03, FULL, &admins),
                (0, 0x03, FULL, &system),
                (0, 0x03, RX, &users()),
                (0, 0x0B, MODIFY, &auth),
                (0, 0x00, FILE_APPEND_DATA, &auth),
            ],
        );
        assert!(evaluate(&root, Role::Ancestor, &user()).is_ok());
        // The same root cannot hold the executable directly.
        assert!(matches!(
            evaluate(&root, Role::ExecutableDir, &user()),
            Err(AclError::UntrustedWriter(_))
        ));
    }

    #[test]
    fn untrusted_writers_are_rejected() {
        let admins = Sid::administrators();
        let writable = security(admins.clone(), &[(0, 0, MODIFY, &users())]);
        for role in [Role::Executable, Role::ExecutableDir, Role::Ancestor] {
            assert!(matches!(
                evaluate(&writable, role, &user()),
                Err(AclError::UntrustedWriter(s)) if s == users()
            ));
        }
        let add_file = security(admins.clone(), &[(0, 0, RX | FILE_WRITE_DATA, &users())]);
        assert!(evaluate(&add_file, Role::ExecutableDir, &user()).is_err());
        assert!(evaluate(&add_file, Role::Ancestor, &user()).is_ok());
        let delete_child = security(admins, &[(0, 0, RX | FILE_DELETE_CHILD, &everyone())]);
        assert!(evaluate(&delete_child, Role::Ancestor, &user()).is_err());
    }

    #[test]
    fn owner_must_be_trusted() {
        let owned_by_users = security(users(), &[]);
        assert!(matches!(
            evaluate(&owned_by_users, Role::Executable, &user()),
            Err(AclError::UntrustedOwner(_))
        ));
        let by_installer = security(Sid::trusted_installer(), &[]);
        assert!(evaluate(&by_installer, Role::Ancestor, &user()).is_ok());
        assert!(evaluate(&by_installer, Role::PrivateDir, &user()).is_err());
        assert!(evaluate(&security(user(), &[]), Role::Executable, &user()).is_ok());
    }

    #[test]
    fn fail_closed_cases() {
        let mut null = security(Sid::administrators(), &[]);
        null.dacl = None;
        assert!(matches!(
            evaluate(&null, Role::Executable, &user()),
            Err(AclError::NullDacl)
        ));
        let callback = security(Sid::administrators(), &[(0x09, 0, RX, &users())]);
        assert!(matches!(
            evaluate(&callback, Role::Executable, &user()),
            Err(AclError::UnknownAce(0x09))
        ));
        let mut reparse = security(user(), &[]);
        reparse.reparse_point = true;
        assert!(matches!(
            evaluate(&reparse, Role::Ancestor, &user()),
            Err(AclError::ReparsePoint)
        ));
        let mut share = security(user(), &[]);
        share.persistent_acls = false;
        assert!(matches!(
            evaluate(&share, Role::PrivateDir, &user()),
            Err(AclError::NotLocal)
        ));
        // A deny entry never makes it fail.
        let deny = security(user(), &[(0x01, 0, FULL, &everyone())]);
        assert!(evaluate(&deny, Role::PrivateDir, &user()).is_ok());
    }

    #[test]
    fn private_dir_counts_any_access_and_inherit_only() {
        let (system, admins) = (Sid::local_system(), Sid::administrators());
        let private = security(
            user(),
            &[
                (0, 0x03, FULL, &user()),
                (0, 0x03, FULL, &system),
                (0, 0x03, FULL, &admins),
            ],
        );
        assert!(evaluate(&private, Role::PrivateDir, &user()).is_ok());
        let readable = security(user(), &[(0, 0, RX, &users())]);
        assert!(matches!(
            evaluate(&readable, Role::PrivateDir, &user()),
            Err(AclError::UntrustedAccess(_))
        ));
        let inherited_only = security(user(), &[(0, 0x09, RX, &users())]);
        assert!(evaluate(&inherited_only, Role::PrivateDir, &user()).is_err());
        assert!(evaluate(&inherited_only, Role::Executable, &user()).is_ok());
    }

    #[test]
    fn malformed_acls_are_rejected() {
        let good = acl(&[(0, 0, RX, &users())]);
        assert!(parse_acl(&good).is_ok());
        for cut in 0..good.len() {
            assert!(parse_acl(&good[..cut]).is_err(), "truncated at {cut}");
        }
        let mut short_ace = good.clone();
        short_ace[10] = 4;
        assert!(parse_acl(&short_ace).is_err());
        let mut sid_overflow = good.clone();
        sid_overflow[17] = 15;
        assert!(parse_acl(&sid_overflow).is_err());
        let mut more_aces = good;
        more_aces[4] = 2;
        assert!(parse_acl(&more_aces).is_err());
    }
}
