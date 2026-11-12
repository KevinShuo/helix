use super::{implementation::*, *};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, fs, io::Read};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_conflicts_index_parser_preserves_literal_paths_and_modes() {
        let oid = "a".repeat(64);
        let input = format!(
            "120000 {oid} 1\t-tabs\tand\nlines[1]\0{mode} {oid} 2\t-tabs\tand\nlines[1]\0",
            mode = "160000"
        );
        let files = parse_unmerged(input.as_bytes()).unwrap();
        assert_eq!(files[0].path, Path::new("-tabs\tand\nlines[1]"));
        assert_eq!(files[0].stages[0].as_ref().unwrap().mode, 0o120000);
        assert_eq!(files[0].stages[1].as_ref().unwrap().mode, 0o160000);
        assert!(files[0].stages[2].is_none());
        assert!(parse_unmerged(format!("100644 {oid} 0\tx\0").as_bytes()).is_err());
    }

    mod merge {
        use super::*;
        use crate::merge::{ConflictChoice, MergeMode, MergeModel};

        async fn candidate(
            base: Option<&str>,
            current: &str,
            incoming: &str,
            working: &str,
        ) -> crate::merge::MergeCandidate {
            let dir = tempfile::tempdir().unwrap();
            let output = std::process::Command::new("git")
                .args(["init", "-q"])
                .arg(dir.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            fs::write(dir.path().join("file"), working).unwrap();
            let repo = GitRepository::open(dir.path().to_owned(), true)
                .await
                .unwrap();
            let file = GitConflictFile {
                path: "file".into(),
                status: if base.is_some() { "UU" } else { "AA" }.into(),
                stages: [None, None, None],
            };
            let operation = ConflictOperation::Merge {
                head: "a".repeat(40),
                parents: vec!["b".repeat(40)],
            };
            let data = GitConflictData {
                version: ConflictVersion {
                    root: repo.root.clone(),
                    git_dir: repo.git_dir.clone(),
                    file: file.clone(),
                    operation: operation.clone(),
                    working: Some(working.as_bytes().to_vec()),
                    attributes: Vec::new(),
                },
                file,
                operation,
                sources: [
                    base.map(|s| s.as_bytes().to_vec()),
                    Some(current.as_bytes().to_vec()),
                    Some(incoming.as_bytes().to_vec()),
                ],
                working: Some(working.as_bytes().to_vec()),
                format: Some(MergeTextFormat::detect(working.as_bytes()).unwrap()),
                support: ConflictSupport::Text,
            };
            let candidate = repo.merge_candidate(&data).await.unwrap();
            assert_eq!(
                fs::read(dir.path().join("file")).unwrap(),
                working.as_bytes()
            );
            assert!(!repo.git_dir.join("index").exists());
            candidate
        }

        #[tokio::test]
        async fn merge_git_candidate_matches_standard_markers_without_writes() {
            let candidate = candidate(
                Some("before\nvalue=1\nafter\n"),
                "before\nvalue=2\nafter\n",
                "before\nvalue=3\nafter\n",
                "before\n<<<<<<< HEAD\nvalue=2\n=======\nvalue=3\n>>>>>>> topic\nafter\n",
            )
            .await;
            assert!(candidate.matches_working);
            assert_eq!(candidate.blocks.len(), 1);
            assert_eq!(candidate.result, "before\nvalue=1\nafter\n");
        }

        #[tokio::test]
        async fn merge_eof_without_newline() {
            let candidate = candidate(
                Some("before\nbase"),
                "before\ncurrent",
                "before\nincoming",
                "before\n<<<<<<< HEAD\ncurrent\n=======\nincoming\n>>>>>>> topic\n",
            )
            .await;
            assert!(candidate.matches_working);
            let model = MergeModel::from_candidate(candidate);
            assert_eq!(model.initial_result(), "before\nbase");
            for (choice, expected) in [
                (ConflictChoice::Current, "before\ncurrent"),
                (ConflictChoice::Incoming, "before\nincoming"),
                (ConflictChoice::Both, "before\ncurrent\nincoming"),
            ] {
                let mut text = model.initial_result().clone();
                model
                    .choice(0, choice, text.slice(..))
                    .unwrap()
                    .apply(&mut text);
                assert_eq!(text, expected);
            }
        }

        #[tokio::test]
        async fn merge_more_than_127_blocks() {
            let (mut base, mut current, mut incoming, mut working) =
                (String::new(), String::new(), String::new(), String::new());
            for i in 0..130 {
                base.push_str(&format!("base-{i}\n"));
                current.push_str(&format!("current-{i}\n"));
                incoming.push_str(&format!("incoming-{i}\n"));
                working.push_str(&format!(
                    "<<<<<<< HEAD\ncurrent-{i}\n=======\nincoming-{i}\n>>>>>>> topic\n"
                ));
                let context = format!("keep-{i}-a\nkeep-{i}-b\nkeep-{i}-c\nkeep-{i}-d\n");
                for text in [&mut base, &mut current, &mut incoming, &mut working] {
                    text.push_str(&context);
                }
            }
            let candidate = candidate(Some(&base), &current, &incoming, &working).await;
            assert_eq!(candidate.blocks.len(), 130);
            assert!(candidate.matches_working);
            assert_eq!(candidate.result, base.as_str());
        }

        #[tokio::test]
        async fn merge_git_aa_and_literal_marker_lines() {
            let left = "<<<<<<<<<<<< helix-current\ncurrent\n";
            let right = "<<<<<<<<<<<< helix-current\nincoming\n";
            let working = "<<<<<<< HEAD\n<<<<<<<<<<<< helix-current\ncurrent\n=======\n<<<<<<<<<<<< helix-current\nincoming\n>>>>>>> topic\n";
            let candidate = candidate(None, left, right, working).await;
            assert!(candidate.matches_working);
            assert_eq!(candidate.blocks.len(), 1);
            assert_eq!(candidate.result, "");
            assert_eq!(candidate.sources.current(), left);
        }

        #[tokio::test]
        async fn merge_git_manual_work_falls_back() {
            let candidate = candidate(
                Some("base\n"),
                "current\n",
                "incoming\n",
                "manually resolved\n",
            )
            .await;
            assert!(!candidate.matches_working);
            let model = MergeModel::from_candidate(candidate);
            assert_eq!(model.initial_result(), "manually resolved\n");
            assert_eq!(model.mode(), MergeMode::WholeFileReview);
            assert!(!model.ready_to_apply());
        }

        #[tokio::test]
        async fn merge_git_clean_nonconflicting_changes() {
            let candidate = candidate(
                Some("left\nkeep\nright\n"),
                "new-left\nkeep\nright\n",
                "left\nkeep\nnew-right\n",
                "new-left\nkeep\nnew-right\n",
            )
            .await;
            assert!(candidate.blocks.is_empty());
            assert!(candidate.matches_working);
            assert_eq!(candidate.result, "new-left\nkeep\nnew-right\n");
        }

        #[tokio::test]
        async fn merge_git_bom_crlf_candidate_preserves_format() {
            let base = "\u{feff}before\r\nbase\r\n";
            let working = "\u{feff}before\r\n<<<<<<< HEAD\r\ncurrent\r\n=======\r\nincoming\r\n>>>>>>> topic\r\n";
            let candidate = candidate(
                Some(base),
                "\u{feff}before\r\ncurrent\r\n",
                "\u{feff}before\r\nincoming\r\n",
                working,
            )
            .await;
            assert!(candidate.matches_working);
            assert_eq!(candidate.result, "before\nbase\n");
            let format = MergeTextFormat::detect(working.as_bytes()).unwrap();
            assert_eq!(
                format.encode(candidate.result.slice(..)).unwrap(),
                base.as_bytes()
            );
        }
    }
}

fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.is_symlink(),
        "Special files require external resolution"
    );
    ensure!(
        metadata.len() <= MAX_MERGE_BYTES as u64,
        "File exceeds the 2 MiB merge limit"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take((MAX_MERGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_MERGE_BYTES,
        "File exceeds the 2 MiB merge limit"
    );
    Ok(Some(bytes))
}

fn parse_unmerged(bytes: &[u8]) -> Result<Vec<GitConflictFile>> {
    let mut files: BTreeMap<PathBuf, [Option<ConflictStage>; 3]> = BTreeMap::new();
    for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .context("Invalid unmerged index record")?;
        let mut header = std::str::from_utf8(&record[..tab])?.split(' ');
        let mode = u32::from_str_radix(header.next().context("Missing index mode")?, 8)?;
        let oid = header.next().context("Missing index object")?;
        ensure!(valid_id(oid), "Invalid index object id");
        let stage: usize = header.next().context("Missing index stage")?.parse()?;
        ensure!(
            (1..=3).contains(&stage) && header.next().is_none(),
            "Invalid index stage"
        );
        let path = path_from_bytes(&record[tab + 1..])?;
        let entry = files.entry(path).or_default();
        ensure!(entry[stage - 1].is_none(), "Duplicate index stage");
        entry[stage - 1] = Some(ConflictStage {
            mode,
            oid: oid.into(),
        });
    }
    Ok(files
        .into_iter()
        .map(|(path, stages)| {
            let present = stages.each_ref().map(Option::is_some);
            let status = match present {
                [true, true, true] => "UU",
                [false, true, true] => "AA",
                [true, true, false] => "UD",
                [true, false, true] => "DU",
                [true, false, false] => "DD",
                [false, true, false] => "AU",
                _ => "UA",
            }
            .into();
            GitConflictFile {
                path,
                stages,
                status,
            }
        })
        .collect())
}

fn unsupported_attributes(bytes: &[u8]) -> Result<()> {
    let fields: Vec<_> = bytes.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
    ensure!(fields.len().is_multiple_of(3), "Invalid Git attributes");
    for record in fields.chunks_exact(3) {
        let (name, value) = (record[1], record[2]);
        let unset = value == b"unspecified" || value == b"unset";
        let supported = match name {
            b"filter" | b"working-tree-encoding" => unset,
            b"merge" => value == b"unspecified" || value == b"set" || value == b"text",
            b"diff" => value != b"unset",
            _ => true,
        };
        ensure!(
            supported,
            "Git attribute {}={} requires external resolution",
            String::from_utf8_lossy(name),
            String::from_utf8_lossy(value)
        );
    }
    Ok(())
}

impl GitRepository {
    /// Build a draft using only temporary inputs. Neither the index nor the
    /// working file is changed; stale-snapshot validation belongs to apply.
    pub async fn merge_candidate(
        &self,
        data: &GitConflictData,
    ) -> Result<crate::merge::MergeCandidate> {
        use crate::merge::{candidate_from_output, marker_size, MergeSources};
        ensure!(
            data.support == ConflictSupport::Text,
            "This conflict requires external resolution"
        );
        let decode = |bytes: &[u8]| MergeTextFormat::detect(bytes)?.decode(bytes);
        let base = data.sources[0]
            .as_deref()
            .map(decode)
            .transpose()?
            .unwrap_or_default();
        let current = decode(
            data.sources[1]
                .as_deref()
                .context("Missing current source")?,
        )?;
        let incoming = decode(
            data.sources[2]
                .as_deref()
                .context("Missing incoming source")?,
        )?;
        let sources = MergeSources::new(base, current, incoming);
        let size = marker_size(&sources);
        let dir = tempfile::tempdir()?;
        let paths = [
            dir.path().join("current"),
            dir.path().join("base"),
            dir.path().join("incoming"),
        ];
        // fs::write closes each handle before Git opens it (including Windows).
        for (path, text) in
            paths
                .iter()
                .zip([sources.current(), sources.base(), sources.incoming()])
        {
            fs::write(path, text.to_string())?;
        }
        let mut command = args(&[
            "merge-file",
            "--stdout",
            "--diff3",
            "-L",
            "helix-current",
            "-L",
            "helix-base",
            "-L",
            "helix-incoming",
        ]);
        command.push(format!("--marker-size={size}").into());
        command.push("--".into());
        command.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
        let output = self.run(&command, QUERY_TIMEOUT).await?;
        ensure!(
            matches!(output.status.code(), Some(0..=127)),
            "Git merge-file failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let text = std::str::from_utf8(&output.stdout)
            .context("Git generated a non-UTF-8 merge result")?;
        let working = data.working.as_deref().map(decode).transpose()?;
        let working = working.map(|text| text.to_string());
        let candidate = candidate_from_output(sources, text, size, working.as_deref())?;
        ensure!(
            output.status.success() == candidate.blocks.is_empty(),
            "Git merge-file output did not match its exit status"
        );
        let format = data.format.context("Missing working file text format")?;
        format.encode(candidate.result.slice(..))?;
        Ok(candidate)
    }

    async fn conflict_operation(&self, has_unmerged: bool) -> Result<ConflictOperation> {
        if self.git_dir.join("rebase-merge").exists() || self.git_dir.join("rebase-apply").exists()
        {
            return Ok(ConflictOperation::Rebase);
        }
        if self.git_dir.join("CHERRY_PICK_HEAD").exists() {
            return Ok(ConflictOperation::CherryPick);
        }
        if let Some(bytes) = read_bounded(&self.git_dir.join("MERGE_HEAD"))? {
            let parents: Vec<String> = std::str::from_utf8(&bytes)?
                .lines()
                .map(str::to_owned)
                .collect();
            ensure!(
                !parents.is_empty() && parents.iter().all(|id| valid_id(id)),
                "Invalid MERGE_HEAD"
            );
            return Ok(ConflictOperation::Merge {
                head: self.head().await?.0.context("Missing merge HEAD")?,
                parents,
            });
        }
        Ok(if has_unmerged {
            ConflictOperation::Other
        } else {
            ConflictOperation::None
        })
    }

    pub async fn conflicts(&self) -> Result<GitConflictSnapshot> {
        let bytes = self
            .checked(&args(&["ls-files", "--unmerged", "-z"]))
            .await?;
        let files = parse_unmerged(&bytes)?;
        let operation = self.conflict_operation(!files.is_empty()).await?;
        Ok(GitConflictSnapshot { operation, files })
    }

    async fn merge_attributes(&self, path: &Path) -> Result<Vec<u8>> {
        let mut command = args(&[
            "check-attr",
            "-z",
            "filter",
            "merge",
            "working-tree-encoding",
            "diff",
            "text",
            "eol",
            "--",
        ]);
        command.push(path.as_os_str().to_owned());
        self.checked(&command).await
    }

    pub async fn conflict_file(&self, path: &Path) -> Result<GitConflictData> {
        let path = self.relative(path)?;
        let snapshot = self.conflicts().await?;
        let file = snapshot
            .files
            .into_iter()
            .find(|f| f.path == path)
            .context("File is no longer conflicted; refresh the conflict list")?;
        let attributes = self.merge_attributes(&path).await?;
        let full_path = self.root.join(&path);
        let mut reason = None;
        if !snapshot.operation.is_merge() {
            reason = Some(
                "This operation requires external resolution; only ordinary merge is editable"
                    .into(),
            );
        } else if !matches!(file.status.as_str(), "UU" | "AA") {
            reason = Some("Deletion or path conflicts require external resolution".into());
        } else if file
            .stages
            .iter()
            .flatten()
            .any(|s| !matches!(s.mode, 0o100644 | 0o100755))
        {
            reason = Some("Symlinks and submodules require external resolution".into());
        }
        if let Err(error) = unsupported_attributes(&attributes) {
            reason.get_or_insert_with(|| error.to_string());
        }
        let working = if !self.contains_path(&full_path)? {
            reason.get_or_insert_with(|| "File resolves outside this worktree".into());
            None
        } else {
            match read_bounded(&full_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    reason.get_or_insert_with(|| error.to_string());
                    None
                }
            }
        };
        if working.is_none() {
            reason.get_or_insert_with(|| "Working file is missing or cannot be read safely".into());
        }
        let mut sources = [None, None, None];
        if reason.is_none() {
            for (source, stage) in sources.iter_mut().zip(&file.stages) {
                if let Some(stage) = stage {
                    let size = self.checked(&args(&["cat-file", "-s", &stage.oid])).await?;
                    let size: usize = std::str::from_utf8(&size)?.trim().parse()?;
                    if size > MAX_MERGE_BYTES {
                        reason = Some("Source exceeds the 2 MiB merge limit".into());
                        break;
                    }
                    let bytes = self
                        .checked(&args(&["cat-file", "blob", &stage.oid]))
                        .await?;
                    if let Err(error) = MergeTextFormat::detect(&bytes) {
                        reason = Some(error.to_string());
                        break;
                    }
                    *source = Some(bytes);
                }
            }
        }
        let format = working
            .as_ref()
            .and_then(|bytes| match MergeTextFormat::detect(bytes) {
                Ok(format) => Some(format),
                Err(error) => {
                    reason.get_or_insert_with(|| error.to_string());
                    None
                }
            });
        let support = reason.map_or(ConflictSupport::Text, |reason| {
            ConflictSupport::InspectOnly { reason }
        });
        let version = ConflictVersion {
            root: self.root.clone(),
            git_dir: self.git_dir.clone(),
            file: file.clone(),
            operation: snapshot.operation.clone(),
            working: working.clone(),
            attributes,
        };
        Ok(GitConflictData {
            file,
            operation: snapshot.operation,
            sources,
            working,
            format,
            support,
            version,
        })
    }
}
