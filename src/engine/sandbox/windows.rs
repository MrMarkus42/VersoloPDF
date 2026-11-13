//! The engine helper on Windows: an AppContainer process in a job.
//!
//! An AppContainer process has no access to the user's files, the registry
//! of the user or the network (no capabilities are granted) and runs with
//! low integrity. The job allows no further processes, limits memory, keeps
//! the helper away from the clipboard and desktop and ends it with the
//! program. Hardening policies forbid remote and dynamically generated
//! code, unknown fonts and extension DLLs.
//!
//! Windows only lets an AppContainer process load files that are shared
//! with it, so the program file and PDFium are copied into
//! `%LOCALAPPDATA%\VersoloPDF\engine\<version>` and that folder is shared
//! with the container (read and execute only).

#![allow(unsafe_code)]

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GENERIC_EXECUTE, GENERIC_READ, HANDLE, HANDLE_FLAG_INHERIT, LocalFree,
    SetHandleInformation,
};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT,
    SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, FreeSid, GetTokenInformation, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY,
    TokenIsAppContainer,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS,
    JOB_OBJECT_UILIMIT_EXITWINDOWS, JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES,
    JOB_OBJECT_UILIMIT_READCLIPBOARD, JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken,
    PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

use super::{Helper, Process};

const CONTAINER: &str = "VersoloPDF.PdfEngine";

// Process creation hardening (winbase.h, PROCESS_CREATION_MITIGATION_POLICY_*).
const HEAP_TERMINATE: u64 = 1 << 12;
const BOTTOM_UP_ASLR: u64 = 1 << 16;
const HIGH_ENTROPY_ASLR: u64 = 1 << 20;
const STRICT_HANDLE_CHECKS: u64 = 1 << 24;
const EXTENSION_POINT_DISABLE: u64 = 1 << 32;
const PROHIBIT_DYNAMIC_CODE: u64 = 1 << 36;
const FONT_DISABLE: u64 = 1 << 48;
const IMAGE_LOAD_NO_REMOTE: u64 = 1 << 52;
const IMAGE_LOAD_PREFER_SYSTEM32: u64 = 1 << 60;
/// PROCESS_CREATION_CHILD_PROCESS_RESTRICTED.
const CHILD_PROCESS_RESTRICTED: u32 = 1;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn last_error(what: &str) -> std::io::Error {
    let e = std::io::Error::last_os_error();
    std::io::Error::new(e.kind(), format!("{what}: {e}"))
}

/// A SID allocated by Windows, freed on drop.
struct Sid(PSID);

impl Drop for Sid {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated by Create/DeriveAppContainer…, freed once.
            unsafe { FreeSid(self.0) };
        }
    }
}

fn container_sid() -> std::io::Result<Sid> {
    let name = wide(OsStr::new(CONTAINER));
    let display = wide(OsStr::new("VersoloPDF PDF engine"));
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: valid strings; `sid` receives an allocated SID.
    let hr = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            display.as_ptr(),
            display.as_ptr(),
            std::ptr::null(),
            0,
            &mut sid,
        )
    };
    if hr >= 0 {
        return Ok(Sid(sid));
    }
    // HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS): the profile is there already.
    let exists = (hr as u32) == (0x8007_0000 | ERROR_ALREADY_EXISTS);
    if !exists {
        return Err(std::io::Error::other(format!("CreateAppContainerProfile: 0x{:08x}", hr as u32)));
    }
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: as above.
    let hr = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
    if hr < 0 {
        return Err(std::io::Error::other(format!("DeriveAppContainerSid: 0x{:08x}", hr as u32)));
    }
    Ok(Sid(sid))
}

/// Shares `dir` (and what is in it) with the container: read and execute.
fn share_with(dir: &Path, sid: &Sid) -> std::io::Result<()> {
    let path = wide(dir.as_os_str());
    let mut old: *mut ACL = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: out pointers; the descriptor is freed below.
    let err = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if err != 0 {
        return Err(std::io::Error::from_raw_os_error(err as i32));
    }
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_READ | GENERIC_EXECUTE,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
            ptstrName: sid.0.cast(),
        },
    };
    let mut new: *mut ACL = std::ptr::null_mut();
    // SAFETY: `old` belongs to `descriptor`, still alive; `new` is freed below.
    let err = unsafe { SetEntriesInAclW(1, &access, old, &mut new) };
    let result = if err == 0 {
        // SAFETY: valid path and ACL.
        let err = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                new,
                std::ptr::null(),
            )
        };
        if err == 0 { Ok(()) } else { Err(std::io::Error::from_raw_os_error(err as i32)) }
    } else {
        Err(std::io::Error::from_raw_os_error(err as i32))
    };
    // SAFETY: allocated by the calls above, freed once.
    unsafe {
        if !new.is_null() {
            LocalFree(new.cast());
        }
        if !descriptor.is_null() {
            LocalFree(descriptor);
        }
    }
    result
}

/// The folder the container may read: program file and PDFium, copied
/// once per version.
fn engine_dir(exe: &Path, sid: &Sid) -> std::io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("LOCALAPPDATA not set"))?
        .join("VersoloPDF")
        .join("engine");
    let meta = std::fs::metadata(exe)?;
    let stamp = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let name = format!("{}-{}-{stamp}", env!("CARGO_PKG_VERSION"), meta.len());
    let dir = base.join(&name);
    let library = super::super::pdfium_lib::library_search_dirs()
        .into_iter()
        .map(|d| pdfium_render::prelude::Pdfium::pdfium_platform_library_name_at_path(&d))
        .find(|p| p.is_file())
        .ok_or_else(|| std::io::Error::other("PDFium not found"))?;
    let target_exe = dir.join("versolopdf-engine.exe");
    let target_lib = dir.join("pdfium.dll");
    if !(target_exe.is_file() && target_lib.is_file()) {
        std::fs::create_dir_all(&dir)?;
        share_with(&dir, sid)?;
        for (from, to) in [(exe, &target_exe), (library.as_path(), &target_lib)] {
            let part = to.with_extension("part");
            std::fs::copy(from, &part)?;
            std::fs::rename(&part, to)?;
        }
        // Copies of earlier versions are no longer needed.
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                if entry.file_name() != name.as_str() {
                    std::fs::remove_dir_all(entry.path()).ok();
                }
            }
        }
    } else {
        share_with(&dir, sid)?;
    }
    Ok(dir)
}

struct WinProcess {
    process: HANDLE,
    job: HANDLE,
}

// SAFETY: the handles are only used through `kill`, which needs `&mut`.
unsafe impl Send for WinProcess {}

impl Process for WinProcess {
    fn exit_code(&mut self) -> Option<i64> {
        use windows_sys::Win32::System::Threading::GetExitCodeProcess;
        if self.process.is_null() {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: a valid process handle owned by this value.
        unsafe {
            WaitForSingleObject(self.process, 2000);
            (GetExitCodeProcess(self.process, &mut code) != 0).then_some(i64::from(code))
        }
    }

    fn kill(&mut self) {
        // SAFETY: handles owned by this value, closed once.
        unsafe {
            if !self.process.is_null() {
                TerminateProcess(self.process, 1);
                WaitForSingleObject(self.process, 5000);
                CloseHandle(self.process);
                self.process = std::ptr::null_mut();
            }
            if !self.job.is_null() {
                CloseHandle(self.job);
                self.job = std::ptr::null_mut();
            }
        }
    }
}

impl Drop for WinProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Closes a handle when dropped (unless taken).
struct Owned(HANDLE);

impl Owned {
    fn take(&mut self) -> HANDLE {
        std::mem::replace(&mut self.0, std::ptr::null_mut())
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: owned handle, closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn pipe() -> std::io::Result<(Owned, Owned)> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: out pointers for two new handles.
    if unsafe { CreatePipe(&mut read, &mut write, &sa, 0) } == 0 {
        return Err(last_error("CreatePipe"));
    }
    Ok((Owned(read), Owned(write)))
}

fn job() -> std::io::Result<Owned> {
    // SAFETY: plain calls with valid, initialised structures.
    unsafe {
        let job = Owned(CreateJobObjectW(std::ptr::null(), std::ptr::null()));
        if job.0.is_null() {
            return Err(last_error("CreateJobObject"));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.ProcessMemoryLimit = 6 << 30;
        if SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(last_error("job limits"));
        }
        let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_EXITWINDOWS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
        };
        if SetInformationJobObject(
            job.0,
            JobObjectBasicUIRestrictions,
            (&ui as *const JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
            std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
        ) == 0
        {
            return Err(last_error("job UI limits"));
        }
        Ok(job)
    }
}

/// Container and folder, prepared once per program run.
struct Prepared {
    sid: Sid,
    dir: PathBuf,
}

// SAFETY: the SID is only read after it was created.
unsafe impl Send for Prepared {}
unsafe impl Sync for Prepared {}

fn prepared(exe: &Path) -> std::io::Result<&'static Prepared> {
    static PREPARED: std::sync::Mutex<Option<&'static Prepared>> = std::sync::Mutex::new(None);
    let mut slot = PREPARED.lock().map_err(|_| std::io::Error::other("lock"))?;
    if let Some(p) = *slot {
        return Ok(p);
    }
    let sid = container_sid()?;
    let dir = engine_dir(exe, &sid)?;
    let p: &'static Prepared = Box::leak(Box::new(Prepared { sid, dir }));
    *slot = Some(p);
    Ok(p)
}

pub fn spawn(exe: &Path) -> std::io::Result<Helper> {
    let prepared = prepared(exe)?;
    let (sid, dir) = (&prepared.sid, &prepared.dir);
    let program = dir.join("versolopdf-engine.exe");
    let (child_in, mut parent_out) = pipe()?;
    let (mut parent_in, child_out) = pipe()?;
    // SAFETY: the program's ends must not be inherited.
    unsafe {
        SetHandleInformation(parent_out.0, HANDLE_FLAG_INHERIT, 0);
        SetHandleInformation(parent_in.0, HANDLE_FLAG_INHERIT, 0);
    }
    let job = job()?;

    let mut capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: sid.0,
        Capabilities: std::ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let handles: [HANDLE; 2] = [child_in.0, child_out.0];
    let child_policy: u32 = CHILD_PROCESS_RESTRICTED;
    let mitigation: u64 = HEAP_TERMINATE
        | BOTTOM_UP_ASLR
        | HIGH_ENTROPY_ASLR
        | STRICT_HANDLE_CHECKS
        | EXTENSION_POINT_DISABLE
        | PROHIBIT_DYNAMIC_CODE
        | FONT_DISABLE
        | IMAGE_LOAD_NO_REMOTE
        | IMAGE_LOAD_PREFER_SYSTEM32;

    // SAFETY: all structures and buffers below live until CreateProcessW
    // has returned; handles are owned and closed by `Owned`.
    unsafe {
        let mut size = 0usize;
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 4, 0, &mut size);
        let mut buffer = vec![0u8; size];
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = buffer.as_mut_ptr().cast();
        if InitializeProcThreadAttributeList(list, 4, 0, &mut size) == 0 {
            return Err(last_error("attribute list"));
        }
        let set = |attribute: usize, value: *const c_void, len: usize| {
            UpdateProcThreadAttribute(list, 0, attribute, value, len, std::ptr::null_mut(), std::ptr::null())
        };
        let ok = set(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            (&mut capabilities as *mut SECURITY_CAPABILITIES).cast(),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
        ) != 0
            && set(
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(&handles),
            ) != 0
            && set(
                PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY as usize,
                (&child_policy as *const u32).cast(),
                std::mem::size_of::<u32>(),
            ) != 0
            && set(
                PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY as usize,
                (&mitigation as *const u64).cast(),
                std::mem::size_of::<u64>(),
            ) != 0;
        if !ok {
            let err = last_error("process attributes");
            DeleteProcThreadAttributeList(list);
            return Err(err);
        }

        let mut info: STARTUPINFOEXW = std::mem::zeroed();
        info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        info.StartupInfo.hStdInput = child_in.0;
        info.StartupInfo.hStdOutput = child_out.0;
        info.StartupInfo.hStdError = std::ptr::null_mut();
        info.lpAttributeList = list;

        let application = wide(program.as_os_str());
        let mut command: Vec<u16> =
            wide(OsStr::new(&format!("\"{}\" {}", program.display(), super::super::host::ENGINE_ARG)));
        let cwd = wide(dir.as_os_str());
        let mut process: PROCESS_INFORMATION = std::mem::zeroed();
        let created = CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            std::ptr::null(),
            cwd.as_ptr(),
            &info.StartupInfo,
            &mut process,
        );
        let create_error = last_error("CreateProcess");
        DeleteProcThreadAttributeList(list);
        drop(buffer);
        if created == 0 {
            return Err(create_error);
        }
        let thread = Owned(process.hThread);
        let mut proc = WinProcess { process: process.hProcess, job: std::ptr::null_mut() };
        if AssignProcessToJobObject(job.0, proc.process) == 0 {
            let err = last_error("AssignProcessToJobObject");
            proc.kill();
            return Err(err);
        }
        let mut job = job;
        proc.job = job.take();
        if ResumeThread(thread.0) == u32::MAX {
            let err = last_error("ResumeThread");
            proc.kill();
            return Err(err);
        }
        drop(thread);
        // The child's ends belong to the child now.
        drop(child_in);
        drop(child_out);
        let to = std::fs::File::from_raw_handle(parent_out.take());
        let from = std::fs::File::from_raw_handle(parent_in.take());
        Ok(Helper::new(Box::new(to), Box::new(from), Box::new(proc)))
    }
}

/// In the helper: checks that it runs in the AppContainer.
pub fn lockdown() -> Result<String, String> {
    let mut token: HANDLE = std::ptr::null_mut();
    let mut is_container = 0u32;
    let mut len = 0u32;
    // SAFETY: plain calls with out pointers; the token is closed below.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err("no process token".into());
        }
        let ok = GetTokenInformation(
            token,
            TokenIsAppContainer,
            (&mut is_container as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            &mut len,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err("token information not available".into());
        }
    }
    if is_container != 0 {
        Ok("Windows AppContainer: no user files, no network; job: no new processes".into())
    } else {
        Err("not running in an AppContainer".into())
    }
}
