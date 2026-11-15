//! Linux 启动时只加载界面实际需要的少量系统字体。
//!
//! GPUI 在 Linux 上用 fontdb 扫描系统全部字体。字体多、缓存冷时，这一步能占掉
//! 启动的大部分时间。这里在创建 GPUI 之前生成一个最小的 fontconfig 配置，只把
//! UI 需要的系统字体文件链接进去；找不到需要的字体时完全不干预，退回原来的扫描。
//!
//! 配置存放在 `$XDG_CACHE_HOME/gistpic-creator/fontconfig`（通常是
//! `~/.cache/gistpic-creator/fontconfig`），第一次启动时生成，之后直接复用。
//! 不写系统目录，也不在每次启动时重新生成。

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// 缓存格式版本。生成规则变化时改这里，旧缓存会自动重建。
    const CACHE_VERSION: &str = "1";

    /// 持有原来的 `FONTCONFIG_FILE`，退出时恢复；缓存目录保留给下次启动复用。
    pub struct FontBootstrap {
        previous: Option<OsString>,
    }

    impl Drop for FontBootstrap {
        fn drop(&mut self) {
            unsafe {
                match self.previous.take() {
                    Some(value) => std::env::set_var("FONTCONFIG_FILE", value),
                    None => std::env::remove_var("FONTCONFIG_FILE"),
                }
            }
        }
    }

    /// 应用精简字体配置。返回 `None` 表示保持系统默认配置。
    pub fn apply() -> Option<FontBootstrap> {
        // 用户或桌面已经指定了字体配置时不要覆盖。
        let previous = std::env::var_os("FONTCONFIG_FILE");
        if previous.is_some() {
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            log::debug!("已设置 FONTCONFIG_FILE，跳过字体启动优化");
            return None;
        }

        let Some(cache_root) = cache_root() else {
            log::warn!("拿不到缓存目录，字体启动优化跳过");
            return None;
        };
        if cache_is_valid(&cache_root) {
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            log::info!("启动字体配置复用缓存：{}", cache_root.display());
        } else if generate(&cache_root).is_some() {
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            log::info!("启动字体配置已生成并缓存：{}", cache_root.display());
        } else {
            log::warn!("字体缓存生成失败，退回系统字体扫描");
            return None;
        }

        unsafe {
            std::env::set_var("FONTCONFIG_FILE", cache_root.join("fonts.conf"));
        }
        Some(FontBootstrap { previous })
    }

    fn cache_root() -> Option<PathBuf> {
        dirs::cache_dir().map(|dir| dir.join("gistpic-creator").join("fontconfig"))
    }

    /// 缓存文件存在、仍然是当前版本、并且至少有一个字体链接没有失效。
    fn cache_is_valid(root: &Path) -> bool {
        let version = match fs::read_to_string(root.join("version")) {
            Ok(version) => version,
            Err(_) => return false,
        };
        if version.trim() != CACHE_VERSION || !root.join("fonts.conf").is_file() {
            return false;
        }

        let entries = match fs::read_dir(root.join("fonts")) {
            Ok(entries) => entries,
            Err(_) => return false,
        };
        let mut has_font = false;
        for entry in entries.flatten() {
            let is_symlink = entry
                .file_type()
                .map(|file_type| file_type.is_symlink())
                .unwrap_or(false);
            if is_symlink && fs::metadata(entry.path()).is_ok() {
                has_font = true;
            }
        }
        has_font
    }

    fn generate(root: &Path) -> Option<()> {
        // `Noto Sans` 是 GPUI 在 Linux 的默认回退字体之一；没有它就不要做精简，
        // 否则 `.SystemUIFont` 可能解析不到任何字体。
        let latin = match_font("Noto Sans", Some("Noto Sans"))?;
        // 中文界面和用户文件名需要中文字体；按系统中文默认字体取一个。
        let cjk = match_font(":lang=zh-cn", None)?;

        let mut files = vec![latin];
        if files[0] != cjk {
            files.push(cjk);
        }

        let fonts_dir = root.join("fonts");
        let cache_dir = root.join("cache");
        fs::create_dir_all(root).ok()?;
        fs::create_dir_all(&cache_dir).ok()?;
        let _ = fs::remove_dir_all(&fonts_dir);
        fs::create_dir_all(&fonts_dir).ok()?;

        for (index, source) in files.iter().enumerate() {
            let extension = Path::new(source)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("ttf");
            let target = fonts_dir.join(format!("font-{index}.{extension}"));
            std::os::unix::fs::symlink(source, &target).ok()?;
        }

        let config = format!(
            "<?xml version=\"1.0\"?>\n\
             <!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n\
             <fontconfig>\n\
               <dir>{}</dir>\n\
               <cachedir>{}</cachedir>\n\
             </fontconfig>\n",
            escape_xml(&fonts_dir.to_string_lossy()),
            escape_xml(&cache_dir.to_string_lossy())
        );
        fs::write(root.join("fonts.conf"), config).ok()?;
        // 版本号最后写：中途失败时缓存会被视为无效，下次重启重新生成。
        fs::write(root.join("version"), CACHE_VERSION).ok()?;
        Some(())
    }

    /// 用 `fc-match` 找字体文件；`expected_family` 用于确认匹配结果确实是该字体族。
    fn match_font(pattern: &str, expected_family: Option<&str>) -> Option<String> {
        let output = Command::new("fc-match")
            .args(["--format=%{file}\t%{family}", pattern])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8(output.stdout).ok()?;
        let mut parts = text.splitn(2, '\t');
        let path = parts.next()?.trim();
        let family = parts.next().unwrap_or("").trim();
        if path.is_empty() || !Path::new(path).is_file() {
            return None;
        }
        if let Some(expected) = expected_family
            && !family.split(',').any(|name| name.trim() == expected)
        {
            return None;
        }
        Some(path.to_string())
    }

    fn escape_xml(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
}

#[cfg(target_os = "linux")]
pub use linux::{FontBootstrap, apply};

#[cfg(not(target_os = "linux"))]
pub struct FontBootstrap;

#[cfg(not(target_os = "linux"))]
pub fn apply() -> Option<FontBootstrap> {
    None
}
