//! Ensure a crashed helper cannot leave orphaned GPU processes behind.
use std::sync::atomic::{AtomicBool, Ordering};

static MANAGER: AtomicBool = AtomicBool::new(false);

pub(super) fn initialize() -> anyhow::Result<()> {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{
                JobObjects::{
                    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                },
                Threading::GetCurrentProcess,
            },
        };
        // Assign ourselves before spawning children: they inherit this job,
        // including during startup. Windows closes the handle if we are killed.
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        anyhow::ensure!(!job.is_null(), "Could not create AI model process job");
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&info) as u32,
        ) == 0
            || AssignProcessToJobObject(job, GetCurrentProcess()) == 0
        {
            let error = std::io::Error::last_os_error();
            CloseHandle(job);
            return Err(error.into());
        }
        // Deliberately process-lifetime, not inherited as a handle. Closing it
        // here would kill the manager itself as well as its model children.
    }
    MANAGER.store(true, Ordering::Release);
    Ok(())
}

pub(super) fn configure_child(_command: &mut tokio::process::Command) {
    #[cfg(target_os = "linux")]
    if MANAGER.load(Ordering::Acquire) {
        let parent = std::process::id();
        unsafe {
            _command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() as u32 != parent {
                    return Err(std::io::Error::other(
                        "AI manager exited during model startup",
                    ));
                }
                Ok(())
            });
        }
    }
}
