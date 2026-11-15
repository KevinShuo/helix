use crate::handlers::ai_test::runner;
use anyhow::{ensure, Context, Result};
use helix_view::{
    ai_test::RunStage,
    test::{Declaration, Outcome, RunRequest, Scope, TestCase},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Debug)]
struct Entry {
    name: String,
    filter: Option<String>,
    path: PathBuf,
    line: usize,
}

fn exact_regex(name: &str) -> String {
    let mut out = String::from("^");
    for c in name.chars() {
        if ".^$*+?()[]{}|\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('$');
    out
}
fn entries(
    value: &Value,
    locations: &[(PathBuf, Declaration)],
    targets: &HashMap<PathBuf, Vec<PathBuf>>,
) -> Vec<Entry> {
    value["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let name = v["name"].as_str()?.to_owned();
            let command: Vec<_> = v["command"]
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .collect();
            let filter = command
                .iter()
                .find_map(|a| a.strip_prefix("--gtest_filter="))
                .map(str::to_owned);
            let sources = command
                .iter()
                .find_map(|arg| targets.get(&super::normalized(arg)));
            let matches: Vec<_> = filter
                .as_ref()
                .map(|f| {
                    locations
                        .iter()
                        .filter(|(path, d)| {
                            d.matches_gtest(f) && sources.is_none_or(|files| files.contains(path))
                        })
                        .collect()
                })
                .unwrap_or_default();
            // An ambiguous name must never bind to an arbitrary executable's source.
            let location = if matches.len() == 1 {
                matches.first().copied()
            } else {
                None
            };
            Some(Entry {
                name,
                filter,
                path: location.map(|(p, _)| p.clone()).unwrap_or_default(),
                line: location.map(|(_, d)| d.line).unwrap_or_default(),
            })
        })
        .collect()
}
fn selected(entry: &Entry, scope: &Scope) -> bool {
    match scope {
        Scope::Project => true,
        Scope::Cases(ids) => ids.contains(&entry.name),
        Scope::File(path) => entry.path == *path,
        Scope::Declaration { path, declaration } => {
            entry.path == *path
                && entry
                    .filter
                    .as_ref()
                    .is_some_and(|f| declaration.matches_gtest(f))
        }
    }
}
fn validate_selection(all: &[Entry], selected: &[&Entry]) -> Result<()> {
    let mut counts = HashMap::new();
    for entry in all {
        *counts.entry(entry.name.as_str()).or_insert(0usize) += 1;
    }
    for entry in selected {
        ensure!(
            counts.get(entry.name.as_str()) == Some(&1),
            "CTest 测试名重复：{}；请为不同 GoogleTest 目标设置独立 TEST_PREFIX，避免误跑其他程序",
            entry.name
        );
    }
    Ok(())
}

fn report(xml: &str, mut case: TestCase) -> Result<TestCase> {
    let doc = roxmltree::Document::parse(xml).context("CTest JUnit 报告无效")?;
    let nodes: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("testcase"))
        .collect();
    let node = nodes
        .iter()
        .find(|n| n.attribute("name") == Some(case.id.as_str()))
        .or_else(|| {
            if nodes.len() == 1 && nodes[0].attribute("name").is_none() {
                nodes.first()
            } else {
                None
            }
        })
        .context("CTest 没有报告所选测试的结果")?;
    case.duration = node
        .attribute("time")
        .and_then(|s| s.parse().ok())
        .unwrap_or(case.duration);
    case.outcome = if node.children().any(|n| n.has_tag_name("error")) {
        Outcome::Error
    } else if node.children().any(|n| n.has_tag_name("failure")) {
        Outcome::Failed
    } else if node.children().any(|n| n.has_tag_name("skipped"))
        || node.attribute("status") == Some("notrun")
    {
        Outcome::Skipped
    } else {
        Outcome::Passed
    };
    case.details = node
        .descendants()
        .filter_map(|n| n.text().filter(|_| n.is_text()))
        .collect::<Vec<_>>()
        .join("\n");
    case.details = case.details.chars().take(32_768).collect();
    // GoogleTest locations use `file:line` or `file(line)`, including drive letters.
    for line in case.details.lines() {
        let trimmed = line.trim();
        let location = trimmed
            .strip_suffix(": Failure")
            .or_else(|| trimmed.strip_suffix(": error: Failure"))
            .unwrap_or(trimmed);
        let pair = if let Some((path, n)) = location.rsplit_once(':') {
            n.trim().parse::<usize>().ok().map(|n| (path, n))
        } else {
            None
        }
        .or_else(|| {
            location
                .strip_suffix(')')
                .and_then(|s| s.rsplit_once('('))
                .and_then(|(p, n)| n.parse().ok().map(|n| (p, n)))
        });
        if let Some((path, n)) = pair.filter(|(p, _)| Path::new(p).is_file()) {
            case.failure_path = Some(super::normalized(path));
            case.failure_line = Some(n.saturating_sub(1));
            break;
        }
    }
    Ok(case)
}

fn target_sources(build: &Path, configuration: &str) -> HashMap<PathBuf, Vec<PathBuf>> {
    fn read(path: &Path) -> Option<Value> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
    let reply = build.join(".cmake/api/v1/reply");
    let index = std::fs::read_dir(&reply)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("index-"))
        })
        .max();
    let Some(index) = index.and_then(|p| read(&p)) else {
        return HashMap::new();
    };
    let Some(model) = index["objects"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["kind"] == "codemodel")
        .and_then(|v| v["jsonFile"].as_str())
        .and_then(|p| read(&reply.join(p)))
    else {
        return HashMap::new();
    };
    let source_root = PathBuf::from(model["paths"]["source"].as_str().unwrap_or_default());
    let mut result = HashMap::new();
    for config in model["configurations"].as_array().into_iter().flatten() {
        if config["name"]
            .as_str()
            .is_some_and(|n| !n.is_empty() && n != configuration)
        {
            continue;
        }
        for entry in config["targets"].as_array().into_iter().flatten() {
            let Some(target) = entry["jsonFile"]
                .as_str()
                .and_then(|p| read(&reply.join(p)))
            else {
                continue;
            };
            if target["type"] != "EXECUTABLE" {
                continue;
            }
            let sources: Vec<_> = target["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    let path = PathBuf::from(s["path"].as_str()?);
                    Some(super::normalized(if path.is_absolute() {
                        path
                    } else {
                        source_root.join(path)
                    }))
                })
                .collect();
            for artifact in target["artifacts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["path"].as_str())
            {
                let path = PathBuf::from(artifact);
                result.insert(
                    super::normalized(if path.is_absolute() {
                        path
                    } else {
                        build.join(path)
                    }),
                    sources.clone(),
                );
            }
        }
    }
    result
}
fn locations(root: &Path) -> Vec<(PathBuf, Declaration)> {
    use helix_core::{config::default_lang_loader, syntax::Syntax, Rope};
    let loader = default_lang_loader();
    let mut result = Vec::new();
    // Respect ignore files; bound parsing to likely source files, not build trees.
    let walker = ignore::WalkBuilder::new(root)
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some("build" | "target" | ".git" | "vcpkg_installed" | "_deps")
            )
        })
        .build();
    for entry in walker.filter_map(Result::ok).take(20_000) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if !matches!(extension, "c" | "cc" | "cpp" | "cxx" | "h" | "hpp" | "hxx") {
            continue;
        }
        if entry.metadata().is_ok_and(|m| m.len() > 524_288) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if !source.contains("TEST") {
            continue;
        }
        let text = Rope::from(source);
        if let Ok(syntax) = Syntax::new(
            text.slice(..),
            loader.language_for_name("cpp").unwrap(),
            &loader,
        ) {
            for d in helix_view::test::discovery::discover_syntax(text.slice(..), &syntax, "cpp") {
                result.push((super::normalized(entry.path()), d));
            }
        }
    }
    result
}
async fn step(id: u64, args: Vec<String>, request: &RunRequest, stage: RunStage) -> Result<()> {
    let result = runner::command(
        Path::new("cmake"),
        &args,
        &request.root,
        stage,
        request.config.run_timeout_ms,
    )
    .await;
    super::log(id, result.log.clone()).await;
    ensure!(result.success, "CMake {stage:?} 失败，详见构建日志");
    Ok(())
}
pub(super) async fn run(id: u64, request: &RunRequest) -> Result<()> {
    ensure!(
        request.root.join("CMakeLists.txt").is_file(),
        "未找到 CMakeLists.txt；请先将 GoogleTest 测试接入 CMake/CTest"
    );
    let config = &request.config;
    let build = if let Some(path) = &config.build_directory {
        if path.is_absolute() {
            path.clone()
        } else {
            request.root.join(path)
        }
    } else if let Some(preset) = &config.cmake_preset {
        runner::preset_binary(&request.root, preset)?
    } else {
        request.root.join("build/helix-ai-tests")
    };
    let build = if build.is_absolute() {
        build
    } else {
        request.root.join(build)
    };
    let query = build.join(".cmake/api/v1/query/client-helix-tests");
    tokio::fs::create_dir_all(&query).await?;
    tokio::fs::write(query.join("codemodel-v2"), "").await?;
    super::phase(id, "配置 CMake").await;
    let mut args = vec![
        "-S".into(),
        request.root.to_string_lossy().into_owned(),
        "-B".into(),
        build.to_string_lossy().into_owned(),
        "-DBUILD_TESTING=ON".into(),
    ];
    if let Some(preset) = &config.cmake_preset {
        args.extend(["--preset".into(), preset.clone()]);
    }
    if !build.join("CMakeCache.txt").exists()
        && config.cmake_preset.is_none()
        && !config
            .cmake_args
            .iter()
            .any(|a| a.contains("CMAKE_TOOLCHAIN_FILE") || a == "--toolchain")
    {
        if let Some(root) = std::env::var_os("VCPKG_ROOT") {
            let toolchain = PathBuf::from(root).join("scripts/buildsystems/vcpkg.cmake");
            if toolchain.is_file() {
                args.push(format!("-DCMAKE_TOOLCHAIN_FILE={}", toolchain.display()));
            }
        }
    }
    args.extend(config.cmake_args.clone());
    step(id, args, request, RunStage::Configure).await?;
    super::phase(id, "编译测试").await;
    let mut args = vec![
        "--build".into(),
        build.to_string_lossy().into_owned(),
        "--config".into(),
        config.build_configuration.clone(),
    ];
    if let Some(target) = &config.test_target {
        args.extend(["--target".into(), target.clone()]);
    }
    step(id, args, request, RunStage::Build).await?;
    super::phase(id, "发现 CTest 测试").await;
    let base = vec![
        "--test-dir".into(),
        build.to_string_lossy().into_owned(),
        "-C".into(),
        config.build_configuration.clone(),
    ];
    let mut args = base.clone();
    args.push("--show-only=json-v1".into());
    let (capture, output) = runner::capture(
        Path::new("ctest"),
        &args,
        &request.root,
        RunStage::Test,
        config.run_timeout_ms,
    )
    .await;
    ensure!(capture.success, "CTest 无法列举测试：{}", capture.log);
    let value: Value = serde_json::from_str(&output).context("CTest 测试清单不是有效 JSON")?;
    let root = request.root.clone();
    let locations = tokio::task::spawn_blocking(move || locations(&root)).await?;
    let all = entries(
        &value,
        &locations,
        &target_sources(&build, &config.build_configuration),
    );
    let selected: Vec<_> = all.iter().filter(|e| selected(e, &request.scope)).collect();
    validate_selection(&all, &selected)?;
    ensure!(!selected.is_empty(), "没有匹配的独立测试；单用例/文件运行需要 gtest_discover_tests 注册。仅整程序注册的工程请使用 :test-project");
    if let Scope::Cases(ids) = &request.scope {
        ensure!(
            ids.iter().all(|id| selected.iter().any(|e| e.name == *id)),
            "部分原测试已不存在；请重新运行文件或项目"
        );
    }
    ensure!(selected.len() <= 10_000, "测试数量超过 10000");
    let temp = tempfile::tempdir()?;
    for e in &selected {
        super::publish(
            id,
            TestCase {
                id: e.name.clone(),
                name: e.name.clone(),
                selector: e.filter.clone(),
                path: e.path.clone(),
                line: e.line,
                outcome: Outcome::Queued,
                duration: 0.,
                details: String::new(),
                failure_path: None,
                failure_line: None,
            },
        )
        .await;
    }
    for (index, e) in selected.iter().enumerate() {
        super::phase(id, &format!("运行 {}/{}", index + 1, selected.len())).await;
        let mut case = TestCase {
            id: e.name.clone(),
            name: e.name.clone(),
            selector: e.filter.clone(),
            path: e.path.clone(),
            line: e.line,
            outcome: Outcome::Running,
            duration: 0.,
            details: String::new(),
            failure_path: None,
            failure_line: None,
        };
        super::publish(id, case.clone()).await;
        let xml = temp.path().join(format!("{index}.xml"));
        let mut args = base.clone();
        args.extend([
            "-R".into(),
            exact_regex(&e.name),
            "--output-on-failure".into(),
            "--no-tests=error".into(),
            "--output-junit".into(),
            xml.to_string_lossy().into_owned(),
        ]);
        let start = Instant::now();
        let result = runner::command(
            Path::new("ctest"),
            &args,
            &request.root,
            RunStage::Test,
            config.run_timeout_ms,
        )
        .await;
        case.duration = start.elapsed().as_secs_f64();
        case.details = result.log.clone();
        let parsed = std::fs::read_to_string(xml)
            .ok()
            .and_then(|xml| report(&xml, case.clone()).ok());
        case = parsed.unwrap_or_else(|| {
            case.outcome = Outcome::Error;
            case
        });
        if !result.success && matches!(case.outcome, Outcome::Passed | Outcome::Skipped) {
            case.outcome = Outcome::Error;
            case.details.push_str(&result.log);
        }
        super::publish(id, case).await;
        super::log(id, result.log).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_ctest_names_cannot_widen_a_single_entry_selection() {
        let value = serde_json::json!({"tests":[
            {"name":"Same","command":["one.exe","--gtest_filter=Suite.Works"]},
            {"name":"Same","command":["two.exe","--gtest_filter=Suite.Works"]}
        ]});
        let entries = entries(&value, &[], &HashMap::new());
        assert!(validate_selection(&entries, &[&entries[0]]).is_err());
        assert!(validate_selection(&entries[..1], &[&entries[0]]).is_ok());
    }
    #[test]
    fn file_api_generated_source_is_relative_to_source_root() {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("out");
        let reply = build.join(".cmake/api/v1/reply");
        std::fs::create_dir_all(&reply).unwrap();
        for (name, value) in [
            (
                "index-1.json",
                serde_json::json!({"objects":[{"kind":"codemodel","jsonFile":"model.json"}]}),
            ),
            (
                "model.json",
                serde_json::json!({"paths":{"source":dir.path()},"configurations":[{"name":"Debug","targets":[{"jsonFile":"target.json"}]}]}),
            ),
            (
                "target.json",
                serde_json::json!({"type":"EXECUTABLE","sources":[{"path":"out/generated.cpp","isGenerated":true}],"artifacts":[{"path":"cases.exe"}]}),
            ),
        ] {
            std::fs::write(reply.join(name), value.to_string()).unwrap();
        }
        let targets = target_sources(&build, "Debug");
        assert_eq!(
            targets[&super::super::normalized(build.join("cases.exe"))],
            vec![super::super::normalized(build.join("generated.cpp"))]
        );
    }
    #[test]
    fn ctest_regex_and_junit_preserve_exact_identity_and_skip() {
        assert_eq!(
            exact_regex("Suite.Test/0 [a+b]"),
            "^Suite\\.Test/0 \\[a\\+b\\]$"
        );
        let case: TestCase = serde_json::from_str(r#"{"id":"x","name":"x"}"#).unwrap();
        assert_eq!(report(r#"<testsuite><testcase time="0.2"><skipped message="skip"/></testcase></testsuite>"#, case.clone()).unwrap().outcome, Outcome::Skipped);
        assert_eq!(
            report(
                r#"<testsuite><testcase><failure>bad &lt; value</failure></testcase></testsuite>"#,
                case
            )
            .unwrap()
            .details,
            "bad < value"
        );
    }
    #[test]
    fn ctest_fixture_success_must_not_hide_target_failure() {
        let case: TestCase =
            serde_json::from_str(r#"{"id":"Suite.Bad","name":"Suite.Bad"}"#).unwrap();
        let xml = r#"<testsuite><testcase name="setup"/><testcase name="Suite.Bad"><failure>assertion</failure></testcase></testsuite>"#;
        assert_eq!(report(xml, case).unwrap().outcome, Outcome::Failed);
    }
    #[test]
    fn duplicate_gtest_names_without_target_mapping_are_not_bound_to_first_file() {
        let d = Declaration {
            start_line: 0,
            line: 0,
            end_line: 1,
            name: "Works".into(),
            suite: Some("Suite".into()),
            class: false,
        };
        let locations = vec![
            (PathBuf::from("a.cpp"), d.clone()),
            (PathBuf::from("b.cpp"), d),
        ];
        let value = serde_json::json!({"tests":[{"name":"one","command":["one.exe","--gtest_filter=Suite.Works"]}]});
        assert!(entries(&value, &locations, &HashMap::new())[0]
            .path
            .as_os_str()
            .is_empty());
        let target = HashMap::from([(
            super::super::normalized("one.exe"),
            vec![PathBuf::from("b.cpp")],
        )]);
        assert_eq!(
            entries(&value, &locations, &target)[0].path,
            PathBuf::from("b.cpp")
        );
    }
}
