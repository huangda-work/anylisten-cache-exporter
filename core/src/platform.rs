use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub enum Platform {
    MacOS,
    Windows,
    Linux,
}

pub fn candidate_roots(platform: Platform, home: &Path, config: &Path) -> Vec<PathBuf> {
    match platform {
        Platform::MacOS => vec![home.join("Library/Application Support/any-listen")],
        Platform::Windows => vec![
            config.join("any-listen"),
            home.join("AppData/Roaming/any-listen"),
        ],
        Platform::Linux => vec![config.join("any-listen"), home.join(".config/any-listen")],
    }
}

pub fn discover_data_root(override_root: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(root) = override_root {
        return if valid_data_root(root) {
            Ok(root.to_path_buf())
        } else {
            Err(format!(
                "所选目录不是 Any Listen 数据目录：{}",
                root.display()
            ))
        };
    }
    let home = dirs::home_dir().ok_or("无法定位用户目录")?;
    let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
    let platform = if cfg!(target_os = "macos") {
        Platform::MacOS
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    };
    candidate_roots(platform, &home, &config)
        .into_iter()
        .find(|root| valid_data_root(root))
        .ok_or_else(|| "未找到 Any Listen 缓存目录，请手动选择 Any Listen 数据目录".into())
}

pub fn settings_path() -> Result<PathBuf, String> {
    dirs::config_dir()
        .map(|directory| directory.join("anylisten-cache-exporter/settings.json"))
        .ok_or_else(|| "无法定位配置目录".into())
}

pub fn valid_data_root(root: &Path) -> bool {
    root.join("Partitions/view-main/Cache/Cache_Data").is_dir()
}
