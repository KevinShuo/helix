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
