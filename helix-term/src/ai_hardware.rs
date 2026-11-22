//! GPU capacity used when choosing AI providers. Never sum adapters or shared RAM.
use helix_view::editor::Config;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

static VRAM: OnceLock<u64> = OnceLock::new();
static LAPTOP: OnceLock<bool> = OnceLock::new();

pub(crate) fn checked_vram() -> Option<u64> {
    VRAM.get().copied()
}

pub(crate) fn configure(config: &mut Config) -> anyhow::Result<()> {
    config.load_local_model_catalog();
    if config.ai.enabled
        && config.ai_completion.enabled
        && config.ai_completion.provider.is_none()
        && config.ai.auto_local_provider.is_some()
    {
        let vram = *VRAM.get_or_init(dedicated_vram);
        let large_model_ready = config
            .ai
            .auto_local_provider
            .as_ref()
            .and_then(|name| config.ai.providers.get(name))
            .is_some_and(|p| p.fits_dedicated_vram(vram));
        if vram < u64::from(config.ai.min_local_vram_gib) << 30 || !large_model_ready {
            let root = std::env::var_os("HELIX_AI_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::current_exe()
                        .ok()?
                        .parent()
                        .map(|p| p.join("helix-ai"))
                });
            if let Some(root) = root.filter(|p| p.is_absolute()) {
                if let Err(err) = apply_compact_model(config, &root, available_ram()) {
                    log::warn!("Could not use compact completion bundle: {err:#}");
                }
            }
        }
    }
    config.apply_ai_host(|| {
        (
            *VRAM.get_or_init(dedicated_vram),
            *LAPTOP.get_or_init(is_laptop),
        )
    })?;
    config.validate_ai()
}

#[cfg(all(windows, not(feature = "integration")))]
fn is_laptop() -> bool {
    use windows_sys::Win32::System::Power::{
        PlatformRoleMobile, PlatformRoleSlate, PowerDeterminePlatformRoleEx, POWER_PLATFORM_ROLE_V2,
    };
    // Windows reports the OEM platform role independently of current AC power.
    let role = unsafe { PowerDeterminePlatformRoleEx(POWER_PLATFORM_ROLE_V2) };
    role == PlatformRoleMobile || role == PlatformRoleSlate
}

#[cfg(all(target_os = "linux", not(feature = "integration")))]
fn is_laptop() -> bool {
    use std::fs;
    let chassis = fs::read_to_string("/sys/class/dmi/id/chassis_type")
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok());
    match chassis {
        Some(8..=11 | 14 | 30..=32) => true,
        None | Some(0..=2) => fs::read_dir("/sys/class/power_supply")
            .into_iter()
            .flatten()
            .flatten()
            .any(|supply| {
                let path = supply.path();
                fs::read_to_string(path.join("type")).is_ok_and(|s| s.trim() == "Battery")
                    && !fs::read_to_string(path.join("scope")).is_ok_and(|s| s.trim() == "Device")
            }),
        _ => false,
    }
}

#[cfg(all(target_os = "macos", not(feature = "integration")))]
fn is_laptop() -> bool {
    std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .is_ok_and(|out| {
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("InternalBattery")
        })
}

#[cfg(any(
    feature = "integration",
    not(any(windows, target_os = "linux", target_os = "macos"))
))]
fn is_laptop() -> bool {
    false
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactModel {
    provider: helix_view::ai::AiProviderConfig,
    completion: helix_view::ai_completion::AiCompletionConfig,
}

fn apply_compact_model(config: &mut Config, root: &Path, ram: u64) -> anyhow::Result<()> {
    let source = match std::fs::read_to_string(root.join("inline.toml")) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        result => result?,
    };
    let CompactModel {
        mut provider,
        mut completion,
    } = toml::from_str(&source)?;
    provider.validate()?;
    anyhow::ensure!(
        provider.is_local_completion_model() && provider.gpu_layers == 0,
        "The compact bundle requires a loopback CPU llama.cpp provider"
    );
    let model = root.join(provider.model_path.as_ref().unwrap());
    provider.server_path = root.join(&provider.server_path);
    let weights = std::fs::metadata(&model)?;
    anyhow::ensure!(
        weights.is_file() && provider.server_path.is_file(),
        "Missing model or server"
    );
    anyhow::ensure!(
        weights.len().saturating_add(2 << 30) <= ram,
        "Insufficient available system memory"
    );
    provider.model_path = Some(model);
    if let Ok(cpus) = std::thread::available_parallelism() {
        let limit = cpus.get().saturating_sub(1).max(1) as u32;
        for threads in [
            &mut provider.runtime.threads,
            &mut provider.runtime.threads_batch,
        ]
        .into_iter()
        .flatten()
        {
            *threads = (*threads).min(limit);
        }
    }
    let mut name = "compact-cpu".to_owned();
    while config.ai.providers.contains_key(&name) {
        name.push('-');
    }
    config.ai.providers.insert(name.clone(), provider);
    completion.provider = Some(name);
    completion.enabled = config.ai_completion.enabled;
    completion.local_only = true;
    config.ai_completion = completion;
    Ok(())
}

#[cfg(windows)]
fn available_ram() -> u64 {
    use winapi::um::sysinfoapi::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut info: MEMORYSTATUSEX = std::mem::zeroed();
        info.dwLength = std::mem::size_of_val(&info) as u32;
        if GlobalMemoryStatusEx(&mut info) != 0 {
            info.ullAvailPhys
        } else {
            0
        }
    }
}

#[cfg(target_os = "linux")]
fn available_ram() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("MemAvailable:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        })
        .unwrap_or(0)
        .saturating_mul(1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn available_ram() -> u64 {
    0
}

// The integration backend runs scripted editor sessions against mock endpoints.
#[cfg(feature = "integration")]
fn dedicated_vram() -> u64 {
    u64::MAX
}

#[cfg(all(windows, not(feature = "integration")))]
fn dedicated_vram() -> u64 {
    use std::{mem::MaybeUninit, ptr};
    use winapi::shared::dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };
    use winapi::Interface;

    let mut capacity = 0;
    // Each successful COM call gives us one reference, released once below.
    unsafe {
        let mut factory: *mut IDXGIFactory1 = ptr::null_mut();
        if CreateDXGIFactory1(
            &IDXGIFactory1::uuidof(),
            (&mut factory as *mut *mut IDXGIFactory1).cast(),
        ) < 0
        {
            return 0;
        }
        for index in 0.. {
            let mut adapter = ptr::null_mut();
            if (*factory).EnumAdapters1(index, &mut adapter) < 0 {
                break;
            }
            let mut desc = MaybeUninit::<DXGI_ADAPTER_DESC1>::uninit();
            if (*adapter).GetDesc1(desc.as_mut_ptr()) >= 0 {
                let desc = desc.assume_init();
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE == 0 {
                    capacity = capacity.max(desc.DedicatedVideoMemory as u64);
                }
            }
            (*adapter).Release();
        }
        (*factory).Release();
    }
    log::info!("AI GPU check: largest dedicated video memory is {capacity} bytes");
    capacity
}

#[cfg(all(target_os = "linux", not(feature = "integration")))]
fn dedicated_vram() -> u64 {
    use std::{fs, process::Command};

    let drm = fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|card| fs::read_to_string(card.path().join("device/mem_info_vram_total")).ok())
        .filter_map(|bytes| bytes.trim().parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    // Proprietary NVIDIA drivers need not export the DRM VRAM attribute.
    let nvidia = Command::new("/usr/bin/nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|out| {
            out.lines()
                .filter_map(|v| v.trim().parse::<u64>().ok())
                .max()
        })
        .unwrap_or(0)
        .saturating_mul(1 << 20);
    drm.max(nvidia)
}

#[cfg(all(not(any(windows, target_os = "linux")), not(feature = "integration")))]
fn dedicated_vram() -> u64 {
    // Unknown dedicated capacity follows the low-memory policy.
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_bundle_uses_relative_paths_and_requires_ram_and_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("weights.gguf"), b"weights").unwrap();
        std::fs::write(root.join("server"), b"server").unwrap();
        std::fs::write(
            root.join("inline.toml"),
            r#"
[provider]
type = "llama-cpp"
model = "compact"
model-path = "weights.gguf"
server-path = "server"
endpoint = "http://127.0.0.1:19243/v1/chat/completions"
gpu-layers = 0
[completion]
max-tokens = 48
"#,
        )
        .unwrap();
        let mut config = Config::default();
        config.ai_completion.enabled = true;
        assert!(apply_compact_model(&mut config, root, 1 << 30).is_err());
        assert!(config.ai_completion.provider.is_none());
        apply_compact_model(&mut config, root, 4 << 30).unwrap();
        config.apply_ai_hardware(|| 128 << 20).unwrap();
        let provider = config
            .ai_provider(helix_view::ai::AiFeature::Completion)
            .unwrap();
        assert_eq!(
            provider.model_path.as_deref(),
            Some(root.join("weights.gguf").as_path())
        );
        assert_eq!(provider.server_path, root.join("server"));
        assert!(config.ai_completion.local_only);
        assert_eq!(config.ai_completion.max_tokens, 48);
        std::fs::remove_file(root.join("weights.gguf")).unwrap();
        assert!(apply_compact_model(&mut Config::default(), root, u64::MAX).is_err());
    }
}
