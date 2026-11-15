//! Owned process trees and bounded, independently framed stdout/stderr streams.
use super::parser::Parser;
use anyhow::{Context, Result};
use helix_event::TaskHandle;
use helix_view::problems::BUILD_LINE_LIMIT;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
};

#[cfg(windows)]
mod ownership {
    use super::*;
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_processes: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process: usize,
        peak_job: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, size: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> Handle;
        fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> Handle;
        fn ResumeThread(thread: Handle) -> u32;
    }
    #[repr(C)]
    #[derive(Default)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        thread: u32,
        process: u32,
        base_priority: i32,
        delta_priority: i32,
        flags: u32,
    }
    struct HandleGuard(Handle);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub(super) struct ProcessOwner(isize);
    impl ProcessOwner {
        pub(super) fn new() -> Result<Self> {
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            anyhow::ensure!(
                !job.is_null(),
                "Cannot create compilation process job: {}",
                std::io::Error::last_os_error()
            );
            let owner = Self(job as isize);
            let mut limits = ExtendedLimits::default();
            limits.basic.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            let ok = unsafe {
                SetInformationJobObject(
                    job,
                    9,
                    &limits as *const _ as *const c_void,
                    std::mem::size_of_val(&limits) as u32,
                )
            };
            anyhow::ensure!(
                ok != 0,
                "Cannot set compilation job limits: {}",
                std::io::Error::last_os_error()
            );
            Ok(owner)
        }
        pub(super) fn attach(&mut self, child: &Child) -> Result<()> {
            let process = child
                .raw_handle()
                .context("Missing compilation process handle")?;
            let ok = unsafe { AssignProcessToJobObject(self.0 as Handle, process as Handle) };
            anyhow::ensure!(
                ok != 0,
                "Cannot own compilation process tree: {}",
                std::io::Error::last_os_error()
            );
            // The shell is created suspended, so no descendant can escape the
            // job before assignment. Resume only this owned process's thread.
            let pid = child.id().context("Missing compilation process ID")?;
            let snapshot = unsafe { CreateToolhelp32Snapshot(0x4, 0) };
            anyhow::ensure!(
                snapshot as isize != -1,
                "Cannot enumerate compilation thread: {}",
                std::io::Error::last_os_error()
            );
            let snapshot = HandleGuard(snapshot);
            let mut entry = ThreadEntry {
                size: std::mem::size_of::<ThreadEntry>() as u32,
                ..Default::default()
            };
            let mut found = unsafe { Thread32First(snapshot.0, &mut entry) };
            while found != 0 {
                if entry.process == pid {
                    let thread = unsafe { OpenThread(0x2, 0, entry.thread) };
                    anyhow::ensure!(
                        !thread.is_null(),
                        "Cannot open compilation thread: {}",
                        std::io::Error::last_os_error()
                    );
                    let thread = HandleGuard(thread);
                    anyhow::ensure!(
                        unsafe { ResumeThread(thread.0) } != u32::MAX,
                        "Cannot resume compilation thread: {}",
                        std::io::Error::last_os_error()
                    );
                    return Ok(());
                }
                found = unsafe { Thread32Next(snapshot.0, &mut entry) };
            }
            anyhow::bail!("Cannot find suspended compilation thread");
        }
    }
    impl Drop for ProcessOwner {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0 as Handle);
            }
        }
    }
}

#[cfg(unix)]
mod ownership {
    use super::*;
    pub(super) struct ProcessOwner(Option<u32>);
    impl ProcessOwner {
        pub(super) fn new() -> Result<Self> {
            Ok(Self(None))
        }
        pub(super) fn attach(&mut self, child: &Child) -> Result<()> {
            self.0 = child.id();
            Ok(())
        }
    }
    impl Drop for ProcessOwner {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }
    }
}

pub(super) struct OwnedChild {
    child: Child,
    _owner: ownership::ProcessOwner,
}

pub(super) fn spawn(shell: &[String], command: &str, cwd: &Path) -> Result<OwnedChild> {
    anyhow::ensure!(!shell.is_empty(), "No shell configured");
    let mut owner = ownership::ProcessOwner::new()?;
    let mut process = Command::new(&shell[0]);
    process.args(&shell[1..]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let name = Path::new(&shell[0])
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if name.eq_ignore_ascii_case("cmd") || name.eq_ignore_ascii_case("cmd.exe") {
            // cmd consumes a command line, not CommandLineToArgvW escaping.
            // The outer quotes are cmd's /C command delimiter, preserving a
            // quoted executable followed by independently quoted arguments.
            process.as_std_mut().raw_arg(format!("\"{command}\""));
        } else {
            process.arg(command);
        }
    }
    #[cfg(not(windows))]
    {
        process.arg(command);
    }
    process
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        process.creation_flags(0x08000004);
    } // NO_WINDOW | SUSPENDED
    #[cfg(unix)]
    {
        process.process_group(0);
    }
    let child = process
        .spawn()
        .context("Could not start compilation shell")?;
    owner.attach(&child)?;
    Ok(OwnedChild {
        child,
        _owner: owner,
    })
}

async fn pipe(mut reader: impl AsyncRead + Unpin, cwd: PathBuf, id: u64) -> std::io::Result<()> {
    let mut parser = Parser::new(cwd);
    let mut line = Vec::new();
    let mut overflow = false;
    let mut bytes = [0u8; 8192];
    loop {
        let count = reader.read(&mut bytes).await?;
        let mut log = String::new();
        let mut problems = Vec::new();
        let mut truncated = false;
        for &byte in &bytes[..count] {
            if byte == b'\n' {
                let text = String::from_utf8_lossy(&line);
                if !overflow {
                    problems.extend(parser.push(&text));
                }
                log.push_str(&text);
                log.push('\n');
                if overflow {
                    log.push_str("[compiler line truncated]\n");
                    truncated = true;
                }
                line.clear();
                overflow = false;
            } else if line.len() < BUILD_LINE_LIMIT {
                line.push(byte);
            } else {
                overflow = true;
            }
        }
        if count == 0 && (!line.is_empty() || overflow) {
            let text = String::from_utf8_lossy(&line);
            if !overflow {
                problems.extend(parser.push(&text));
            }
            log.push_str(&text);
            if overflow {
                log.push_str("\n[compiler line truncated]\n");
                truncated = true;
            }
        }
        if !log.is_empty() || !problems.is_empty() {
            crate::job::dispatch(move |editor, _| {
                super::output(editor, id, log, problems, truncated)
            })
            .await;
        }
        if count == 0 {
            return Ok(());
        }
    }
}

pub(super) async fn run(mut process: OwnedChild, cwd: PathBuf, id: u64, handle: TaskHandle) {
    let stdout = process.child.stdout.take().expect("stdout was piped");
    let stderr = process.child.stderr.take().expect("stderr was piped");
    let owner = process._owner;
    let output = async {
        let result = process.child.wait().await;
        // End descendants immediately when the shell exits, even when they
        // inherited stdout/stderr. Then pipe readers can drain to EOF.
        drop(owner);
        result
    };
    let result = tokio::select! {
        biased;
        _ = handle.canceled() => return,
        result = async {
            let (status, _, _) = tokio::try_join!(output, pipe(stdout, cwd.clone(), id), pipe(stderr, cwd, id))?;
            std::io::Result::Ok(status)
        } => result,
    };
    // Closing the job/group also ends any descendants left behind by the shell.
    let status = match result {
        Ok(status) => format!(
            "Finished (exit {})",
            status
                .code()
                .map_or_else(|| "signal".into(), |c| c.to_string())
        ),
        Err(err) => format!("Failed to read compilation output: {err}"),
    };
    crate::job::dispatch(move |editor, _| super::finished(editor, id, status)).await;
}
