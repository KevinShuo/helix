//! Discover local llama.cpp models without loading weights or starting servers.
use super::{AiProviderConfig, AiProviderType, AiSpeculativeDecoding};
use crate::editor::Config;
use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

const MAX_ENTRIES: usize = 8192;
const MAX_DEPTH: usize = 4;
const MAX_MODELS: usize = 128;
const MAX_SHARDS: usize = 512;
const PORT_START: u16 = 22000;
const PORT_COUNT: u16 = 10000;

// llama.cpp uses the first file of <name>-00001-of-000NN.gguf as the entry point.
fn shard(stem: &str) -> Option<(&str, usize, usize)> {
    let (part, count) = stem.rsplit_once("-of-")?;
    let (name, index) = part.rsplit_once('-')?;
    if name.is_empty()
        || index.len() != 5
        || count.len() != 5
        || !index
            .bytes()
            .chain(count.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((name, index.parse().ok()?, count.parse().ok()?))
}

fn file_bytes(path: &Path) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    metadata.is_file().then_some(metadata.len())
}

pub(super) fn weight_bytes(path: &Path) -> Option<u64> {
    let Some((name, index, count)) = path.file_stem().and_then(|s| s.to_str()).and_then(shard)
    else {
        return file_bytes(path);
    };
    if !(1..=MAX_SHARDS).contains(&count) || !(1..=count).contains(&index) {
        return None;
    }
    let extension = path.extension()?.to_str()?;
    (1..=count).try_fold(0u64, |total, n| {
        let part = path.with_file_name(format!("{name}-{n:05}-of-{count:05}.{extension}"));
        total.checked_add(file_bytes(&part)?)
    })
}

fn model_name(path: &Path) -> Option<&str> {
    if !path.extension()?.to_str()?.eq_ignore_ascii_case("gguf") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    if stem.to_ascii_lowercase().contains("mmproj")
        || stem.trim().is_empty()
        || stem.contains(',')
        || stem.chars().any(char::is_control)
    {
        return None;
    }
    match shard(stem) {
        Some((name, 1, count)) if (1..=MAX_SHARDS).contains(&count) => Some(name),
        Some(_) => None,
        None => Some(stem),
    }
}

fn scan(roots: BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
    let mut queue: VecDeque<_> = roots.into_iter().take(64).map(|p| (p, 0)).collect();
    let mut visited = BTreeSet::new();
    let mut files = BTreeSet::new();
    let mut remaining = MAX_ENTRIES;
    while let Some((dir, depth)) = queue.pop_front() {
        if !visited.insert(dir.clone()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        // Skip an oversized directory as a unit instead of selecting an arbitrary
        // subset in filesystem enumeration order.
        let mut entries: Vec<_> = entries.take(remaining + 1).collect();
        if entries.len() > remaining {
            break;
        }
        remaining -= entries.len();
        entries.sort_by_key(|entry| entry.as_ref().ok().map(fs::DirEntry::file_name));
        for entry in entries.into_iter().flatten() {
            let path = entry.path();
            let Ok(ty) = entry.file_type() else { continue };
            if ty.is_dir() && depth < MAX_DEPTH {
                // A junction can be reported as a directory on Windows.
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if entry
                        .metadata()
                        .is_ok_and(|m| m.file_attributes() & 0x400 != 0)
                    {
                        continue;
                    }
                }
                queue.push_back((path, depth + 1));
            } else if (ty.is_file() || ty.is_symlink()) && model_name(&path).is_some() {
                files.insert(path);
            }
        }
    }
    files
}

// Stable across process restarts. The path distinguishes identical model names
// in separate directories and keeps their provider identifiers independent.
fn path_hash(path: &Path) -> u64 {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, &b| {
            (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)
        })
}

impl Config {
    pub fn discover_local_models(&mut self) {
        // Only discovered entries have this runtime flag in the provider map.
        self.ai
            .providers
            .retain(|name, p| !(p.on_demand && name.starts_with("gguf-")));
        let candidates = [
            self.ai.local_generation_provider.as_ref(),
            self.ai_explain.local_provider.as_ref(),
            self.ai_generate.local_provider.as_ref(),
            self.ai_doc.local_provider.as_ref(),
            self.ai_test.local_provider.as_ref(),
            self.ai_commit.local_provider.as_ref(),
            self.ai_merge.local_provider.as_ref(),
            self.ai_explain.provider.as_ref(),
            self.ai_generate.provider.as_ref(),
            self.ai_doc.provider.as_ref(),
            self.ai_test.provider.as_ref(),
            self.ai_commit.provider.as_ref(),
            self.ai_merge.provider.as_ref(),
            self.ai.auto_local_provider.as_ref(),
            self.ai.default_provider.as_ref(),
        ];
        let preferred: Vec<_> = candidates
            .into_iter()
            .flatten()
            .filter_map(|name| self.ai.providers.get(name))
            .filter(|p| p.is_local_completion_model())
            .collect();
        let template = preferred
            .first()
            .copied()
            .or_else(|| {
                self.ai
                    .providers
                    .values()
                    .find(|p| p.is_local_completion_model())
            })
            .cloned()
            .unwrap_or_else(|| AiProviderConfig {
                kind: AiProviderType::LlamaCpp,
                ..Default::default()
            });
        let directories: Vec<_> = if self.ai.local_model_directories.is_empty() {
            preferred
                .iter()
                .filter_map(|p| p.model_path.as_ref())
                .chain(template.model_path.as_ref())
                .filter_map(|p| {
                    let path = helix_stdx::path::expand(p);
                    path.parent()
                        .filter(|dir| !dir.as_os_str().is_empty())
                        .map(Path::to_owned)
                })
                .collect()
        } else {
            self.ai
                .local_model_directories
                .iter()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| helix_stdx::path::expand(p).into_owned())
                .collect()
        };
        let roots = directories
            .into_iter()
            .filter_map(|p| fs::canonicalize(p).ok())
            .filter(|p| p.parent().is_some() && p.is_dir())
            .collect();
        let mut known: BTreeSet<_> = self
            .ai
            .providers
            .values()
            .filter_map(|p| p.model_path.as_ref())
            .filter_map(|p| fs::canonicalize(helix_stdx::path::expand(p)).ok())
            .collect();
        let mut ports: BTreeSet<_> = self
            .ai
            .providers
            .values()
            .chain(std::iter::once(&AiProviderConfig::from_legacy(
                &self.ai_completion,
            )))
            .filter_map(AiProviderConfig::listener_identity)
            .map(|(_, port)| port)
            .collect();
        let mut added = 0;
        for file in scan(roots) {
            let Ok(identity) = fs::canonicalize(&file) else {
                continue;
            };
            let name = model_name(&file).unwrap();
            if known.contains(&identity) || weight_bytes(&file).is_none() {
                continue;
            }
            known.insert(identity);
            let hash = path_hash(&file);
            let key = format!("gguf-{name}-{hash:016x}");
            if self.ai.providers.contains_key(&key) {
                continue;
            }
            let slot = (hash % u64::from(PORT_COUNT)) as u16;
            let Some(port) = (0..PORT_COUNT)
                .map(|offset| PORT_START + (slot + offset) % PORT_COUNT)
                .find(|port| ports.insert(*port))
            else {
                break;
            };
            let mut provider = template.clone();
            provider.endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
            provider.model = name.to_owned();
            provider.model_path = Some(file);
            provider.on_demand = true;
            // The new weights may have a different chat template and no MTP head.
            provider.thinking = false;
            provider.thinking_budget = None;
            provider.reasoning_effort = None;
            provider.runtime.speculative_decoding = AiSpeculativeDecoding::None;
            self.ai.providers.insert(key, provider);
            added += 1;
            if added == MAX_MODELS {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn catalog_is_stable_and_shards_are_one_model() {
        let root = tempdir().unwrap();
        let dir = root.path().join("snapshot");
        fs::create_dir(&dir).unwrap();
        let first = dir.join("coder-00001-of-00002.gguf");
        let second = dir.join("coder-00002-of-00002.gguf");
        fs::write(&first, b"GGUFone").unwrap();
        fs::write(&second, b"GGUFtwo").unwrap();
        fs::write(dir.join("mmproj-coder-f16.gguf"), b"GGUF").unwrap();
        fs::write(dir.join("other-00001-of-00003.gguf"), b"GGUF").unwrap();
        fs::write(dir.join("orphan-00002-of-00003.gguf"), b"GGUF").unwrap();
        fs::write(dir.join("model.gguf.part"), b"GGUF").unwrap();
        fs::write(root.path().join("coder.GGUF"), b"GGUF").unwrap();
        let mut config = Config::default();
        config.ai.local_model_directories = vec![root.path().to_owned(), dir.clone()];
        config.discover_local_models();
        let before = config.ai.providers.clone();
        assert_eq!(before.len(), 2);
        let mut ports = BTreeSet::new();
        for (name, provider) in &before {
            assert!(name.starts_with("gguf-coder-"));
            assert_eq!(provider.model, "coder");
            assert!(provider.is_local_completion_model() && provider.on_demand);
            assert!(ports.insert(provider.listener_identity().unwrap().1));
        }
        assert_eq!(weight_bytes(&second), Some(14));
        config.discover_local_models();
        assert_eq!(config.ai.providers, before);
        fs::remove_file(&second).unwrap();
        assert_eq!(weight_bytes(&first), None);
        config.discover_local_models();
        assert_eq!(config.ai.providers.len(), 1);
        assert!(before
            .iter()
            .any(|(k, p)| config.ai.providers.get(k) == Some(p)));
    }

    #[test]
    fn template_and_explicit_directories_preserve_provider_choices() {
        let base = tempdir().unwrap();
        let extra = tempdir().unwrap();
        let resident = base.path().join("configured.gguf");
        fs::write(&resident, b"GGUF").unwrap();
        fs::write(base.path().join("nearby.gguf"), b"GGUF").unwrap();
        fs::write(extra.path().join("explicit.gguf"), b"GGUF").unwrap();
        let mut template = AiProviderConfig {
            kind: AiProviderType::LlamaCpp,
            model: "configured".into(),
            endpoint: "http://127.0.0.1:22001/v1/chat/completions".into(),
            model_path: Some(resident),
            server_path: base.path().join("custom-server"),
            context_size: 8192,
            ..Default::default()
        };
        template.runtime.speculative_decoding = AiSpeculativeDecoding::Mtp;
        template.runtime.threads = Some(3);
        let mut config = Config::default();
        config.ai_doc.local_provider = Some("configured".into());
        config
            .ai
            .providers
            .insert("configured".into(), template.clone());
        config.discover_local_models();
        assert_eq!(config.ai.providers.len(), 2);
        assert_eq!(config.ai.providers["configured"], template);
        let nearby = config
            .ai
            .providers
            .values()
            .find(|p| p.model == "nearby")
            .unwrap();
        assert_eq!(
            nearby.runtime.speculative_decoding,
            AiSpeculativeDecoding::None
        );
        assert_eq!(nearby.runtime.threads, Some(3));
        assert_eq!(nearby.server_path, template.server_path);
        assert_eq!(nearby.context_size, template.context_size);
        assert_ne!(nearby.endpoint, template.endpoint);

        config.ai.local_model_directories = vec![extra.path().into()];
        config.discover_local_models();
        assert_eq!(config.ai.providers.len(), 2);
        assert!(config.ai.providers.values().any(|p| p.model == "explicit"));
        assert_eq!(config.ai.providers["configured"], template);
    }
}
