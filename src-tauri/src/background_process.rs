use std::ffi::OsStr;

#[cfg(target_os = "windows")]
const WINDOWS_CREATE_NO_WINDOW_FLAG: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const WINDOWS_CREATE_SUSPENDED_FLAG: u32 = 0x0000_0004;

/// 构造不应显示终端窗口的同步子进程。
#[allow(
    clippy::disallowed_methods,
    reason = "这是 std::process::Command 的统一后台构造边界"
)]
pub(crate) fn std_command<S>(program: S) -> std::process::Command
where
    S: AsRef<OsStr>,
{
    let mut command = std::process::Command::new(program);
    apply_std_policy(&mut command);
    command
}

pub(crate) fn configure_std_process_group(command: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(WINDOWS_CREATE_NO_WINDOW_FLAG | WINDOWS_CREATE_SUSPENDED_FLAG);
    }
}

pub(crate) struct StdProcessTree {
    #[cfg(unix)]
    process_group_id: Option<i32>,
    #[cfg(target_os = "windows")]
    job: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(unix)]
pub(crate) fn attach_std_process_tree(
    child: &std::process::Child,
) -> std::io::Result<StdProcessTree> {
    Ok(StdProcessTree {
        process_group_id: i32::try_from(child.id()).ok(),
    })
}

#[cfg(target_os = "windows")]
pub(crate) fn attach_std_process_tree(
    child: &std::process::Child,
) -> std::io::Result<StdProcessTree> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error());
    }

    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    };
    let assigned = configured != 0
        && unsafe { AssignProcessToJobObject(job, child.as_raw_handle().cast()) } != 0;
    if !assigned {
        let error = std::io::Error::last_os_error();
        unsafe {
            CloseHandle(job);
        }
        return Err(error);
    }

    Ok(StdProcessTree { job })
}

#[cfg(target_os = "windows")]
pub(crate) fn resume_std_process(child: &std::process::Child) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }

    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut has_entry = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    let mut result = Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "suspended process thread was not found",
    ));

    while has_entry {
        if entry.th32OwnerProcessID == child.id() {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                result = Err(std::io::Error::last_os_error());
            } else {
                let previous_suspend_count = unsafe { ResumeThread(thread) };
                result = if previous_suspend_count == u32::MAX {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                };
                unsafe {
                    CloseHandle(thread);
                }
            }
            break;
        }
        has_entry = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }

    unsafe {
        CloseHandle(snapshot);
    }
    result
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn resume_std_process(_child: &std::process::Child) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
pub(crate) fn attach_std_process_tree(
    _child: &std::process::Child,
) -> std::io::Result<StdProcessTree> {
    Ok(StdProcessTree {})
}

pub(crate) fn terminate_std_process_tree(
    child: &mut std::process::Child,
    process_tree: &StdProcessTree,
) {
    #[cfg(unix)]
    if let Some(process_group_id) = process_tree.process_group_id {
        let _ = unsafe { libc::kill(-process_group_id, libc::SIGKILL) };
    }

    #[cfg(target_os = "windows")]
    unsafe {
        windows_sys::Win32::System::JobObjects::TerminateJobObject(process_tree.job, 1);
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(target_os = "windows")]
impl Drop for StdProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

/// 构造不应显示终端窗口的异步子进程。
#[cfg(target_os = "windows")]
#[allow(
    clippy::disallowed_methods,
    reason = "这是 tokio::process::Command 的统一后台构造边界"
)]
pub(crate) fn tokio_command<S>(program: S) -> tokio::process::Command
where
    S: AsRef<OsStr>,
{
    let mut command = tokio::process::Command::new(program);
    apply_tokio_policy(&mut command);
    command
}

#[cfg(target_os = "windows")]
fn apply_std_policy(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(WINDOWS_CREATE_NO_WINDOW_FLAG);
}

#[cfg(not(target_os = "windows"))]
fn apply_std_policy(_command: &mut std::process::Command) {}

#[cfg(target_os = "windows")]
fn apply_tokio_policy(command: &mut tokio::process::Command) {
    command.creation_flags(WINDOWS_CREATE_NO_WINDOW_FLAG);
}
