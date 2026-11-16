//! User-local model ownership independent of any one editor process.
mod client;
mod daemon;
mod process;
mod protocol;

pub(crate) use client::{is_registered, with_model, LocalServers};

/// Internal helper entry, before terminal/configuration initialization.
pub async fn run_if_requested() -> anyhow::Result<bool> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--internal-ai-manager")) {
        return Ok(false);
    }
    anyhow::ensure!(
        cfg!(any(windows, target_os = "linux")),
        "Shared AI manager requires Windows or Linux process lifetime protection"
    );
    let root = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("AI manager directory is required"))?;
    anyhow::ensure!(args.next().is_none(), "Unexpected AI manager arguments");
    daemon::run(std::path::Path::new(&root)).await?;
    Ok(true)
}

pub(crate) fn configure_model_child(command: &mut tokio::process::Command) {
    process::configure_child(command);
}
