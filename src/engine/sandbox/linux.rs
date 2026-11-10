//! Restrictions of the engine helper on Linux: Landlock (files) and seccomp
//! (network, processes).

#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use landlock::{
    ABI, Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus,
};

/// Folders with fonts: the only places the helper may read.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        ["/usr/share/fonts", "/usr/local/share/fonts", "/usr/X11R6/lib/X11/fonts", "/etc/fonts"]
            .iter()
            .map(PathBuf::from)
            .collect();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".fonts"));
        dirs.push(home.join(".local/share/fonts"));
    }
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

fn landlock() -> Result<String, String> {
    let abi = ABI::V3;
    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .map_err(|e| e.to_string())?
        .create()
        .map_err(|e| e.to_string())?;
    for dir in font_dirs() {
        let fd = PathFd::new(&dir).map_err(|e| e.to_string())?;
        ruleset =
            ruleset.add_rule(PathBeneath::new(fd, AccessFs::from_read(abi))).map_err(|e| e.to_string())?;
    }
    let status = ruleset.restrict_self().map_err(|e| e.to_string())?;
    match status.ruleset {
        RulesetStatus::FullyEnforced => Ok("Landlock: files only fonts".into()),
        RulesetStatus::PartiallyEnforced => Ok("Landlock (partly): files only fonts".into()),
        RulesetStatus::NotEnforced => Err("Landlock is not available (kernel too old)".into()),
    }
}

fn seccomp() -> Result<String, String> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, TargetArch};
    let denied = [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_clone,
        libc::SYS_clone3,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_fork,
        #[cfg(target_arch = "x86_64")]
        libc::SYS_vfork,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_kill,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
    ];
    let rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> =
        denied.iter().map(|s| (*s, Vec::new())).collect();
    let arch = TargetArch::try_from(std::env::consts::ARCH).map_err(|e| e.to_string())?;
    let filter =
        SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch)
            .map_err(|e| e.to_string())?;
    let program: BpfProgram = filter.try_into().map_err(|e: seccompiler::BackendError| e.to_string())?;
    seccompiler::apply_filter(&program).map_err(|e| e.to_string())?;
    Ok("seccomp: no network, no new processes".into())
}

pub fn lockdown() -> Result<String, String> {
    // No PR_SET_PDEATHSIG: it follows the thread that started the helper
    // (helpers are started in advance by a short-lived thread). The helper
    // ends when the program closes its pipe.
    // SAFETY: plain prctl call without pointers.
    unsafe {
        // Needed for seccomp and Landlock; no new rights through exec either.
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err("no_new_privs failed".into());
        }
    }
    let files = landlock();
    let calls = seccomp();
    match (files, calls) {
        (Ok(a), Ok(b)) => Ok(format!("{a}; {b}")),
        (Err(a), Ok(b)) => Err(format!("{a}; {b}")),
        (Ok(a), Err(b)) => Err(format!("{a}; seccomp failed: {b}")),
        (Err(a), Err(b)) => Err(format!("{a}; seccomp failed: {b}")),
    }
}
