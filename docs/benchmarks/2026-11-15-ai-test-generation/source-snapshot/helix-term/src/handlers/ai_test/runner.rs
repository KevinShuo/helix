//! Bounded argv-only test execution. Never run commands supplied by a model.
use anyhow::{ensure, Context, Result};
use helix_view::ai_test::{AiTestConfig, Project, RunReport, RunStage};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

const LOG_LIMIT: usize = 65_536;

struct ProcessTree(Option<u32>);
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("taskkill.exe")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .creation_flags(0x08000000)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

async fn read_log(mut reader: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buf = [0; 4096];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        result.extend_from_slice(&buf[..n]);
        if result.len() > LOG_LIMIT {
            result.drain(..result.len() - LOG_LIMIT);
        }
    }
    Ok(result)
}

pub(crate) async fn command(
    program: &Path,
    args: &[String],
    root: &Path,
    stage: RunStage,
    timeout_ms: u64,
) -> RunReport {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.as_std_mut().process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return RunReport {
                stage: RunStage::Environment,
                success: false,
                repairable: false,
                log: format!("无法启动 {}：{e}", program.display()),
            }
        }
    };
    let mut tree = ProcessTree(child.id());
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let result = tokio::time::timeout(
        Duration::from_millis(timeout_ms.clamp(100, 600_000)),
        async { tokio::try_join!(child.wait(), read_log(stdout), read_log(stderr)) },
    )
    .await;
    match result {
        Ok(Ok((status, out, err))) => {
            tree.0 = None;
            let log = format!(
                "{} {:?}\n{}\n{}",
                program.display(),
                args,
                String::from_utf8_lossy(&out),
                String::from_utf8_lossy(&err)
            );
            RunReport {
                stage,
                success: status.success(),
                repairable: stage == RunStage::Build && !status.success(),
                log,
            }
        }
        Ok(Err(e)) => RunReport {
            stage,
            success: false,
            repairable: false,
            log: format!("执行失败：{e}"),
        },
        Err(_) => RunReport {
            stage,
            success: false,
            repairable: false,
            log: format!(
                "{} 执行超时（{}ms），已停止进程树",
                program.display(),
                timeout_ms
            ),
        },
    }
}

pub(crate) fn python(root: &Path, config: &AiTestConfig) -> PathBuf {
    if let Some(path) = &config.python {
        return if path.is_absolute() || path.components().count() == 1 {
            path.clone()
        } else {
            root.join(path)
        };
    }
    for name in [".venv", "venv"] {
        let path = root.join(name).join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        });
        if path.is_file() {
            return path;
        }
    }
    if let Some(path) = std::env::var_os("VIRTUAL_ENV") {
        let path = PathBuf::from(path).join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        });
        if path.is_file() {
            return path;
        }
    }
    PathBuf::from("python")
}

fn preset_binary(root: &Path, name: &str) -> Result<PathBuf> {
    let mut presets = Vec::new();
    for file in ["CMakePresets.json", "CMakeUserPresets.json"] {
        if let Ok(text) = std::fs::read_to_string(root.join(file)) {
            let value: serde_json::Value = serde_json::from_str(&text)
                .context("CMake preset JSON 无法解析；请设置 build-directory")?;
            if let Some(array) = value["configurePresets"].as_array() {
                presets.extend(array.clone());
            }
        }
    }
    let mut pending = vec![name.to_owned()];
    let mut visited = Vec::new();
    while let Some(current) = pending.pop() {
        ensure!(
            visited.len() < 32 && !visited.contains(&current),
            "CMake preset 继承循环"
        );
        visited.push(current.clone());
        let preset = presets
            .iter()
            .find(|p| p["name"].as_str() == Some(current.as_str()))
            .context("找不到 configure preset；请设置 build-directory")?;
        if let Some(binary) = preset["binaryDir"].as_str() {
            let binary = binary
                .replace("${sourceDir}", &root.to_string_lossy())
                .replace(
                    "${sourceParentDir}",
                    &root.parent().unwrap_or(root).to_string_lossy(),
                )
                .replace("${presetName}", name)
                .replace(
                    "${sourceDirName}",
                    &root.file_name().unwrap_or_default().to_string_lossy(),
                );
            ensure!(
                !binary.contains('$'),
                "复杂 preset binaryDir 请显式设置 ai-test.build-directory"
            );
            return Ok(PathBuf::from(binary));
        }
        if let Some(parent) = preset["inherits"].as_str() {
            pending.push(parent.to_owned());
        } else if let Some(parents) = preset["inherits"].as_array() {
            pending.extend(
                parents
                    .iter()
                    .rev()
                    .filter_map(|p| p.as_str().map(str::to_owned)),
            );
        }
    }
    anyhow::bail!("configure preset 缺少 binaryDir；请设置 ai-test.build-directory")
}

pub(crate) fn build_directory(project: &Project, config: &AiTestConfig) -> Result<PathBuf> {
    let path = if let Some(path) = &config.build_directory {
        path.clone()
    } else if let Some(preset) = &config.cmake_preset {
        preset_binary(&project.root, preset)?
    } else {
        PathBuf::from(".helix-ai/build")
    };
    super::project::confined(&project.root, &path)
}

pub(crate) async fn run(project: &Project, config: &AiTestConfig) -> RunReport {
    match run_inner(project, config).await {
        Ok(report) => report,
        Err(error) => RunReport {
            stage: RunStage::Environment,
            success: false,
            repairable: false,
            log: error.to_string(),
        },
    }
}

async fn run_inner(project: &Project, config: &AiTestConfig) -> Result<RunReport> {
    if project.language == "python" {
        let py = python(&project.root, config);
        let test = project.test_file.to_string_lossy().into_owned();
        let args = vec![
            "-m".into(),
            "pytest".into(),
            "-q".into(),
            "-rA".into(),
            "--color=no".into(),
            test,
        ];
        let mut report = command(
            &py,
            &args,
            &project.root,
            RunStage::Test,
            config.run_timeout_ms,
        )
        .await;
        if report.success {
            let passed = report
                .log
                .lines()
                .any(|line| line.trim_start().starts_with("PASSED "));
            let skipped = report.log.lines().any(|line| {
                ["SKIPPED ", "XFAIL ", "XPASS "]
                    .iter()
                    .any(|s| line.trim_start().starts_with(s))
            });
            if skipped && !passed {
                report.success = false;
                report
                    .log
                    .push_str("\n本次测试全部跳过或标记为预期失败，不能报告通过。\n");
            }
        }
        if !report.success {
            if report.log.contains("No module named pytest") {
                report.stage = RunStage::Environment;
            } else if report.log.contains("ERROR collecting")
                && [
                    "ImportError",
                    "ModuleNotFoundError",
                    "SyntaxError",
                    "NameError",
                ]
                .iter()
                .any(|e| report.log.contains(e))
            {
                report.repairable = true;
            }
        }
        return Ok(report);
    }
    ensure!(
        !project.target_name.is_empty(),
        "测试草稿尚未接入 CMake，无法自动运行；请先配置项目的 CMake 构建"
    );
    let build = build_directory(project, config)?;
    let mut configure = vec![
        "-S".into(),
        project.root.to_string_lossy().into_owned(),
        "-B".into(),
        build.to_string_lossy().into_owned(),
        "-DBUILD_TESTING=ON".into(),
    ];
    if let Some(preset) = &config.cmake_preset {
        configure.extend(["--preset".into(), preset.clone()]);
    }
    if config.cmake_preset.is_none()
        && !build.join("CMakeCache.txt").is_file()
        && !config
            .cmake_args
            .iter()
            .any(|a| a.contains("CMAKE_TOOLCHAIN_FILE") || a == "--toolchain")
    {
        if let Some(root) = std::env::var_os("VCPKG_ROOT") {
            let toolchain = PathBuf::from(root).join("scripts/buildsystems/vcpkg.cmake");
            if toolchain.is_file() {
                configure.push(format!(
                    "-DCMAKE_TOOLCHAIN_FILE={}",
                    toolchain.to_string_lossy()
                ));
            }
        }
    }
    configure.extend(config.cmake_args.clone());
    let mut logs = String::new();
    let report = command(
        Path::new("cmake"),
        &configure,
        &project.root,
        RunStage::Configure,
        config.run_timeout_ms,
    )
    .await;
    if !report.success {
        return Ok(report);
    }
    logs.push_str(&report.log);
    let args = vec![
        "--build".into(),
        build.to_string_lossy().into_owned(),
        "--config".into(),
        config.build_configuration.clone(),
        "--target".into(),
        project.target_name.clone(),
    ];
    let report = command(
        Path::new("cmake"),
        &args,
        &project.root,
        RunStage::Build,
        config.run_timeout_ms,
    )
    .await;
    if !report.success {
        return Ok(report);
    }
    logs.push_str(&report.log);
    let args = vec![
        "--test-dir".into(),
        build.to_string_lossy().into_owned(),
        "-C".into(),
        config.build_configuration.clone(),
        "--output-on-failure".into(),
        "--no-tests=error".into(),
        "-R".into(),
        project.test_filter.clone(),
    ];
    let mut report = command(
        Path::new("ctest"),
        &args,
        &project.root,
        RunStage::Test,
        config.run_timeout_ms,
    )
    .await;
    logs.push_str(&report.log);
    report.log = logs
        .chars()
        .rev()
        .take(LOG_LIMIT)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_executable_is_environment_failure_without_repair() {
        let dir = tempfile::tempdir().unwrap();
        let report = command(
            &dir.path().join("no-such-program"),
            &[],
            dir.path(),
            RunStage::Build,
            1000,
        )
        .await;
        assert_eq!(report.stage, RunStage::Environment);
        assert!(!report.success && !report.repairable);
    }

    #[tokio::test]
    async fn bounded_logs_keep_the_diagnostic_tail() {
        let data = vec![b'x'; LOG_LIMIT + 100];
        assert_eq!(read_log(&data[..]).await.unwrap().len(), LOG_LIMIT);
    }

    #[test]
    fn preset_inheritance_keeps_its_binary_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakePresets.json"), r#"{"configurePresets":[{"name":"base","binaryDir":"${sourceDir}/out/${presetName}"},{"name":"debug","inherits":"base"}]}"#).unwrap();
        assert_eq!(
            preset_binary(dir.path(), "debug").unwrap(),
            dir.path().join("out/debug")
        );
    }

    #[tokio::test]
    async fn timeout_stops_execution_and_never_triggers_repair() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let (program, args) = (
            Path::new("powershell.exe"),
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 30".into(),
            ],
        );
        #[cfg(not(windows))]
        let (program, args) = (Path::new("sleep"), vec!["30".into()]);
        let started = std::time::Instant::now();
        let report = command(program, &args, dir.path(), RunStage::Build, 200).await;
        assert!(!report.success && !report.repairable);
        assert!(report.log.contains("超时"), "{report:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
