//! Keep Windows Git launchers and their descendants inside one cancelable scope.
use std::io;
use tokio::process::{Child, Command};

pub(crate) struct ProcessTree {
    // This non-inheritable handle is the sole owner of a kill-on-close job.
    // Dropping an execution future therefore also stops descendants that still
    // hold stdout/stderr open, including while the runtime itself shuts down.
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
}

pub(crate) fn spawn(command: &mut Command) -> io::Result<(Child, ProcessTree)> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        let tree = ProcessTree::new()?;
        // Assign before allowing any user code to run: assigning a running
        // launcher leaves a race in which its child escapes the job.
        command
            .kill_on_drop(true)
            .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        let mut child = command.spawn()?;
        let assigned = unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(
                tree.job.as_raw_handle(),
                child
                    .raw_handle()
                    .ok_or_else(|| io::Error::other("Git process handle unavailable"))?,
            )
        };
        if assigned == 0 {
            let error = io::Error::last_os_error();
            let _ = child.start_kill();
            return Err(error);
        }
        resume_initial_thread(
            child
                .id()
                .ok_or_else(|| io::Error::other("Git process ID unavailable"))?,
        )?;
        Ok((child, tree))
    }
    #[cfg(not(windows))]
    {
        Ok((command.spawn()?, ProcessTree {}))
    }
}

#[cfg(windows)]
impl ProcessTree {
    fn new() -> io::Result<Self> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = OwnedHandle::from_raw_handle(handle);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { job })
        }
    }

    fn active(&self) -> io::Result<u32> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            JobObjectBasicAccountingInformation, QueryInformationJobObject,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        };
        unsafe {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            if QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(info.ActiveProcesses)
        }
    }

    fn start_termination(&self) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if self.active()? == 0 {
            return Ok(());
        }
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(crate) fn terminate_blocking(&self, budget: std::time::Duration) -> io::Result<()> {
        self.start_termination()?;
        let deadline = std::time::Instant::now() + budget;
        while self.active()? != 0 {
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Git process tree did not exit",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }

    pub(crate) async fn terminate(&self) -> io::Result<()> {
        use std::time::Duration;
        self.start_termination()?;
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.active()? != 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Ok(())
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Git process tree did not exit"))?
    }
}

#[cfg(windows)]
fn resume_initial_thread(pid: u32) -> io::Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = OwnedHandle::from_raw_handle(snapshot);
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        if Thread32First(snapshot.as_raw_handle(), &mut entry) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut initial = None;
        loop {
            if entry.th32OwnerProcessID == pid && initial.replace(entry.th32ThreadID).is_some() {
                return Err(io::Error::other(
                    "Suspended Git process has multiple initial threads",
                ));
            }
            if Thread32Next(snapshot.as_raw_handle(), &mut entry) == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                    return Err(error);
                }
                break;
            }
        }
        let tid = initial.ok_or_else(|| io::Error::other("Suspended Git thread unavailable"))?;
        let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, tid);
        if thread.is_null() {
            return Err(io::Error::last_os_error());
        }
        let thread = OwnedHandle::from_raw_handle(thread);
        match ResumeThread(thread.as_raw_handle()) {
            1 => Ok(()),
            u32::MAX => Err(io::Error::last_os_error()),
            _ => Err(io::Error::other("Git initial thread was not suspended")),
        }
    }
}
