use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

pub(crate) struct Project {
    pub target: PathBuf,
    pub is_git: bool,
    pub detected: BTreeMap<&'static str, Vec<String>>,
    pub warnings: Vec<String>,
}

const MARKERS: &[(&str, &[&str])] = &[
    ("Cargo.toml", &["rust"]),
    ("package.json", &["node"]),
    ("pyproject.toml", &["python"]),
    ("requirements.txt", &["python"]),
    ("setup.py", &["python"]),
    ("setup.cfg", &["python"]),
    ("Pipfile", &["python"]),
    ("go.mod", &["go"]),
    ("pom.xml", &["java", "maven"]),
    ("build.gradle", &["java", "gradle"]),
    ("build.gradle.kts", &["java", "gradle"]),
    ("settings.gradle", &["java", "gradle"]),
    ("settings.gradle.kts", &["java", "gradle"]),
    ("CMakeLists.txt", &["cpp", "cmake"]),
];

pub(crate) fn detect(start: &Path) -> Result<Project> {
    let start = start
        .canonicalize()
        .with_context(|| format!("Cannot access {}", start.display()))?;
    anyhow::ensure!(
        start.is_dir(),
        "Project directory does not exist: {}",
        start.display()
    );
    let root = git_root(&start)?;
    let is_git = root.is_some();
    let root = root.unwrap_or_else(|| helix_loader::find_workspace_in(&start).0);
    detect_directory(&root, is_git)
}

pub(super) fn git_root(start: &Path) -> Result<Option<PathBuf>> {
    for ancestor in start.ancestors() {
        match fs::symlink_metadata(ancestor.join(".git")) {
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => {
                return Ok(Some(ancestor.to_owned()));
            }
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error).context("Cannot locate Git directory"),
        }
    }
    Ok(None)
}

pub(super) fn detect_directory(root: &Path, is_git: bool) -> Result<Project> {
    let mut entries =
        fs::read_dir(root).with_context(|| format!("Cannot read {}", root.display()))?;
    let mut project = Project {
        target: root.join(".gitignore"),
        is_git,
        detected: BTreeMap::new(),
        warnings: Vec::new(),
    };
    for &(marker, ids) in MARKERS {
        match fs::symlink_metadata(root.join(marker)) {
            Ok(metadata) if metadata.is_file() => {
                for id in ids {
                    project
                        .detected
                        .entry(id)
                        .or_default()
                        .push(marker.to_owned());
                }
            }
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => project
                .warnings
                .push(format!("Cannot inspect {marker}: {error}")),
        }
    }
    for entry in entries.by_ref().take(4096) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                project.warnings.push(format!("Partial detection: {error}"));
                continue;
            }
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let extension = Path::new(name.as_ref())
            .extension()
            .and_then(|s| s.to_str());
        if !matches!(
            extension,
            Some("sln" | "slnx" | "csproj" | "fsproj" | "vbproj")
        ) {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_file() => project
                .detected
                .entry("dotnet")
                .or_default()
                .push(name.into_owned()),
            Ok(_) => (),
            Err(error) => project
                .warnings
                .push(format!("Cannot inspect {name}: {error}")),
        }
    }
    if entries.next().is_some() {
        project.warnings.push("Partial detection: directory scan limited to 4096 entries; select any missing templates manually".into());
    }
    for evidence in project.detected.values_mut() {
        evidence.sort();
        evidence.dedup();
    }
    Ok(project)
}
