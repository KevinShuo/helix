use super::*;
use crate::handlers::ai_doc::target::tests::find;

#[test]
fn python_uses_project_root_and_never_overwrites_handwritten_tests() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname='demo'\n",
    )
    .unwrap();
    let source = dir.path().join("maths.py");
    std::fs::write(&source, "def add(a, b):\n    return a + b\n").unwrap();
    let target = find(
        "python",
        &std::fs::read_to_string(&source).unwrap(),
        "return",
    )
    .unwrap();
    let project = discover(&source, &target, &AiTestConfig::default()).unwrap();
    assert_eq!(project.root, helix_stdx::path::canonicalize(dir.path()));
    assert!(project
        .test_file
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("test_helix_"));
    std::fs::create_dir_all(project.test_file.parent().unwrap()).unwrap();
    std::fs::write(&project.test_file, "# handwritten\n").unwrap();
    assert!(discover(&source, &target, &AiTestConfig::default()).is_err());
}

#[test]
fn cmake_reuses_library_and_prefers_installed_gtest_before_pinned_download() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.20)\nproject(demo C CXX)\nadd_library(maths maths.c)\n",
    )
    .unwrap();
    let source = dir.path().join("maths.c");
    std::fs::write(&source, "int add(int a, int b) { return a + b; }\n").unwrap();
    let target = find("c", &std::fs::read_to_string(&source).unwrap(), "return").unwrap();
    let project = discover(&source, &target, &AiTestConfig::default()).unwrap();
    let build = project
        .build_changes
        .iter()
        .map(|c| c.after.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(build.contains("PRIVATE maths"));
    assert!(
        build.find("find_package(GTest CONFIG QUIET)").unwrap()
            < build.find("FetchContent_Declare").unwrap()
    );
    assert!(build.contains("v1.17.0"));
    assert_eq!(
        build.matches("add_library(maths").count(),
        1,
        "preserve the original library without recompiling a copy"
    );
    let mut config = AiTestConfig::default();
    config.allow_fetch_content = false;
    let project = discover(&source, &target, &config).unwrap();
    assert!(!project
        .build_changes
        .iter()
        .any(|c| c.after.contains("FetchContent_Declare")));
}

#[test]
fn cmake_does_not_treat_comments_or_executables_as_linkable_libraries() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("maths.cpp");
    std::fs::write(&source, "int add(int a, int b) { return a + b; }\n").unwrap();
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        "project(demo CXX)\n# add_library(fake maths.cpp)\nadd_executable(app maths.cpp)\n",
    )
    .unwrap();
    let target = find("cpp", &std::fs::read_to_string(&source).unwrap(), "return").unwrap();
    assert!(discover(&source, &target, &AiTestConfig::default()).is_err());
    assert_eq!(commands("#[=[ add_library(fake x.cpp) ]=]\nmessage(\"add_library(fake x.cpp)\")\nadd_library(real STATIC \"maths.cpp\")").unwrap().len(), 2);
}

#[test]
fn cmake_finds_top_level_project_and_ignores_uncalled_targets() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        "project(demo C)\nadd_subdirectory(src)\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/CMakeLists.txt"), "message(\"(\")\nfunction(unused)\nadd_library(fake maths.c)\nendfunction()\nadd_library(maths maths.c)\n").unwrap();
    let source = dir.path().join("src/maths.c");
    std::fs::write(&source, "int add(int a, int b) { return a + b; }\n").unwrap();
    let target = find("c", &std::fs::read_to_string(&source).unwrap(), "return").unwrap();
    let project = discover(&source, &target, &AiTestConfig::default()).unwrap();
    assert_eq!(project.root, helix_stdx::path::canonicalize(dir.path()));
    assert!(project
        .build_changes
        .iter()
        .any(|c| c.after.contains("PRIVATE maths")));
}

#[test]
fn regeneration_snapshots_unchanged_generated_cmake_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        "project(demo C)\nadd_library(maths maths.c)\n",
    )
    .unwrap();
    let source = dir.path().join("maths.c");
    std::fs::write(&source, "int add(int a, int b) { return a + b; }\n").unwrap();
    let target = find("c", &std::fs::read_to_string(&source).unwrap(), "return").unwrap();
    let first = discover(&source, &target, &AiTestConfig::default()).unwrap();
    for change in changes(&first, "#include <gtest/gtest.h>\n").unwrap() {
        std::fs::create_dir_all(change.before.path.parent().unwrap()).unwrap();
        std::fs::write(change.before.path, change.after).unwrap();
    }
    let again = discover(&source, &target, &AiTestConfig::default()).unwrap();
    assert!(again.build_changes.is_empty());
    for dependency in &first.build_changes {
        assert!(
            again
                .snapshots
                .iter()
                .any(|s| s.path == dependency.before.path),
            "missing {}",
            dependency.before.path.display()
        );
    }
}

#[test]
fn cmake_include_retains_callers_source_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("cmake")).unwrap();
    std::fs::write(
        dir.path().join("CMakeLists.txt"),
        "project(demo C)\ninclude(cmake/Targets.cmake)\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("cmake/Targets.cmake"),
        "add_library(maths maths.c)\n",
    )
    .unwrap();
    let source = dir.path().join("maths.c");
    std::fs::write(&source, "int add(int a, int b) { return a + b; }\n").unwrap();
    let target = find("c", &std::fs::read_to_string(&source).unwrap(), "return").unwrap();
    let project = discover(&source, &target, &AiTestConfig::default()).unwrap();
    assert!(project
        .build_changes
        .iter()
        .any(|c| c.after.contains("PRIVATE maths")));
}

#[test]
fn documented_and_single_line_functions_are_test_targets() {
    use helix_core::{config::default_lang_loader, syntax::Syntax, Rope};
    for (language, source, needle) in [
        (
            "python",
            "def f(a):\n    \"\"\"Existing docs.\"\"\"\n    return a\n",
            "return",
        ),
        ("python", "def f(a): return a\n", "return"),
        (
            "cpp",
            "struct C { int f(int a) { return a; } };\n",
            "return",
        ),
        ("c", "/* docs */\nint f(int a) { return a; }\n", "return"),
    ] {
        let text = Rope::from(source);
        let loader = default_lang_loader();
        let syntax = Syntax::new(
            text.slice(..),
            loader.language_for_name(language).unwrap(),
            &loader,
        )
        .unwrap();
        assert!(
            crate::handlers::ai_doc::target::locate_for_test(
                text.slice(..),
                &syntax,
                source.find(needle).unwrap(),
                language
            )
            .is_ok(),
            "{source}"
        );
    }
}

#[tokio::test]
#[ignore = "requires CMake, C/C++ compiler and installed GTest/vcpkg"]
async fn real_cmake_c_library_links_and_runs_installed_gtest() {
    cmake_smoke(false, false).await;
}

#[tokio::test]
#[ignore = "requires CMake, compiler and network access for pinned FetchContent"]
async fn real_cmake_fetchcontent_fallback_builds_when_installed_search_is_disabled() {
    cmake_smoke(true, false).await;
}

#[tokio::test]
#[ignore = "requires CMake, C/C++ compiler and installed GTest/vcpkg"]
async fn real_cmake_existing_gtest_target_links_new_library_and_keeps_test_prefix() {
    cmake_smoke(false, true).await;
}

async fn cmake_smoke(fetch: bool, existing: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root_cmake = if existing {
        std::fs::write(
            dir.path().join("old_test.cpp"),
            "#include <gtest/gtest.h>\nTEST(Old, Works) { EXPECT_EQ(2, 2); }\n",
        )
        .unwrap();
        "cmake_minimum_required(VERSION 3.20)\nproject(demo C CXX)\nadd_library(maths maths.c)\nfind_package(GTest CONFIG REQUIRED)\nenable_testing()\nadd_executable(existing old_test.cpp)\ntarget_link_libraries(existing GTest::gtest_main)\ninclude(GoogleTest)\ngtest_discover_tests(existing TEST_PREFIX legacy_)\n"
    } else {
        "cmake_minimum_required(VERSION 3.20)\nproject(demo LANGUAGES C)\nadd_library(maths maths.c)\n"
    };
    std::fs::write(dir.path().join("CMakeLists.txt"), root_cmake).unwrap();
    std::fs::write(dir.path().join("maths.h"), "int add(int a, int b);\n").unwrap();
    let source = dir.path().join("maths.c");
    std::fs::write(&source, "int add(int a, int b) { return a + b; }\n").unwrap();
    let target = find("c", &std::fs::read_to_string(&source).unwrap(), "return").unwrap();
    let config = AiTestConfig {
        allow_fetch_content: fetch,
        cmake_args: if fetch {
            vec!["-DCMAKE_DISABLE_FIND_PACKAGE_GTest=ON".into()]
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    let project = discover(&source, &target, &config).unwrap();
    if existing {
        assert_eq!(project.target_name, "existing");
    }
    let code = format!("#include <gtest/gtest.h>\nextern \"C\" {{\n#include \"maths.h\"\n}}\nTEST({}, 相加) {{ EXPECT_EQ(add(2, 3), 5); }}\n", project.test_filter);
    for change in changes(&project, &code).unwrap() {
        std::fs::create_dir_all(change.before.path.parent().unwrap()).unwrap();
        std::fs::write(change.before.path, change.after).unwrap();
    }
    let report = super::super::runner::run(&project, &config).await;
    assert!(report.success, "stage={:?}\n{}", report.stage, report.log);
    std::fs::write(&source, "int add(int a, int b) { return a - b; }\n").unwrap();
    let report = super::super::runner::run(&project, &config).await;
    assert!(
        !report.success && report.log.contains("Expected equality"),
        "{}",
        report.log
    );
}

#[tokio::test]
#[ignore = "requires HELIX_AI_TEST_PYTHON with pytest installed"]
async fn real_pytest_failure_and_zero_collection_are_never_success_or_repairable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.pytest.ini_options]\n",
    )
    .unwrap();
    let source = dir.path().join("maths.py");
    std::fs::write(&source, "def add(a, b):\n    return a + b\n").unwrap();
    let target = find(
        "python",
        &std::fs::read_to_string(&source).unwrap(),
        "return",
    )
    .unwrap();
    let config = AiTestConfig {
        python: Some(
            std::env::var_os("HELIX_AI_TEST_PYTHON")
                .expect("Python path")
                .into(),
        ),
        ..Default::default()
    };
    let project = discover(&source, &target, &config).unwrap();
    std::fs::create_dir_all(project.test_file.parent().unwrap()).unwrap();
    std::fs::write(
        &project.test_file,
        "from maths import add\ndef test_add():\n    assert add(2, 3) == 6\n",
    )
    .unwrap();
    let report = super::super::runner::run(&project, &config).await;
    assert!(!report.success && !report.repairable, "{report:?}");
    assert!(report.log.contains("AssertionError"));
    std::fs::write(&project.test_file, "# no tests\n").unwrap();
    let report = super::super::runner::run(&project, &config).await;
    assert!(!report.success && !report.repairable, "{report:?}");
    std::fs::write(dir.path().join("conftest.py"), "import pytest\n@pytest.fixture(autouse=True)\ndef unavailable_environment():\n    pytest.skip('environment unavailable')\n").unwrap();
    std::fs::write(
        &project.test_file,
        "from maths import add\ndef test_add():\n    assert add(2, 3) == 5\n",
    )
    .unwrap();
    let report = super::super::runner::run(&project, &config).await;
    assert!(
        !report.success && !report.repairable,
        "all-skipped cannot pass: {report:?}"
    );
}
