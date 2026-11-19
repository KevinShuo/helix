use super::super::{
    detect::{detect_directory, git_root},
    rules::Sources,
};
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, HashMap},
    hash::Hash,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};
const SAMPLE_LIMIT: usize = 4096;
const CACHE_TTL: Duration = Duration::from_secs(2);

// Do expensive reads outside the lock. Multiple editor instances share only immutable snapshots.
struct Cache<K, V>(HashMap<K, (Instant, Arc<V>)>);

impl<K: Eq + Hash, V> Cache<K, V> {
    fn new() -> Mutex<Self> {
        Mutex::new(Self(HashMap::new()))
    }

    fn read(
        cache: &Mutex<Self>,
        key: K,
        refresh: bool,
        load: impl FnOnce() -> Option<V>,
    ) -> Option<Arc<V>> {
        if !refresh {
            if let Some((time, value)) = cache.lock().0.get(&key) {
                if time.elapsed() < CACHE_TTL {
                    return Some(value.clone());
                }
            }
        }
        let started = Instant::now();
        let value = Arc::new(load()?);
        let mut cache = cache.lock();
        cache.0.retain(|_, (time, _)| time.elapsed() < CACHE_TTL);
        if cache.0.len() >= 16 {
            cache.0.clear();
        }
        if cache.0.get(&key).is_none_or(|(time, _)| started >= *time) {
            cache.0.insert(key, (started, value.clone()));
        }
        Some(value)
    }
}

type Markers = BTreeMap<&'static str, Vec<String>>;
static MARKERS: LazyLock<Mutex<Cache<PathBuf, Markers>>> = LazyLock::new(Cache::new);

pub(super) fn project_markers(base: &Path, manual: bool) -> Arc<Markers> {
    let get = |path: &Path| {
        Cache::read(&MARKERS, path.to_owned(), manual, || {
            Some(detect_directory(path, false).ok()?.detected)
        })
        .unwrap_or_default()
    };
    let local = get(base);
    if local.is_empty() {
        if let Ok(Some(root)) = git_root(base) {
            if root != base {
                return get(&root);
            }
        }
    }
    local
}

pub(super) struct Entry {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) directory: bool,
    pub(super) regular: bool,
}

pub(super) struct Sample {
    pub(super) entries: Vec<Entry>,
    pub(super) partial: bool,
    pub(super) sources: Sources,
}

type SampleKey = (PathBuf, PathBuf, bool);
static SAMPLES: LazyLock<Mutex<Cache<SampleKey, Sample>>> = LazyLock::new(Cache::new);

pub(super) fn sample(
    base: &Path,
    directory: &Path,
    trust_full: bool,
    refresh: bool,
    cancelled: &impl Fn() -> bool,
) -> Option<Arc<Sample>> {
    Cache::read(
        &SAMPLES,
        (base.into(), directory.into(), trust_full),
        refresh,
        || {
            let mut iter = directory.read_dir().ok()?;
            let mut entries = Vec::new();
            let mut partial = false;
            for item in iter.by_ref().take(SAMPLE_LIMIT) {
                if cancelled() {
                    return None;
                }
                let Ok(item) = item else {
                    partial = true;
                    continue;
                };
                let Ok(kind) = item.file_type() else {
                    partial = true;
                    continue;
                };
                let Ok(name) = item.file_name().into_string() else {
                    continue;
                };
                if name.eq_ignore_ascii_case(".git") || name.contains(['\n', '\r']) {
                    continue;
                }
                entries.push(Entry {
                    path: item.path(),
                    name,
                    directory: kind.is_dir(),
                    regular: kind.is_file(),
                });
            }
            partial |= iter.next().is_some();
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            Some(Sample {
                entries,
                partial,
                sources: Sources::load(base, directory, trust_full),
            })
        },
    )
}
