//! 启动时跳过「配置还在、驱动却用不了」的 Vulkan 驱动。
//!
//! Vulkan 加载器会枚举系统里所有 ICD 配置。如果某个驱动的内核模块没加载，
//! 加载器仍然会去初始化它，等它探测失败才放弃。这台机器上 NVIDIA 显卡绑在
//! vfio-pci 上给虚拟机用，驱动模块平时不加载，但 nvidia-utils 的 ICD 配置还在，
//! 每次启动要为此白等 1.48 秒。
//!
//! 这里在创建 GPUI 之前检查一遍：ICD 配置存在、但该驱动对应的设备节点不存在，
//! 就把这个 ICD 加进 `VK_LOADER_DRIVERS_DISABLE`，让加载器直接跳过。设备节点
//! 出现时（用户手动加载了驱动）就不动它，行为和系统默认一致。

/// ICD 配置文件名 → 判断该驱动是否真的能用的检查点。
///
/// 不能用 `/dev/nvidia0`：那个节点只要内核模块加载就会创建，即使显卡被
/// vfio-pci 之类的驱动占着、实际用不了，节点也会一直在。
/// `/proc/driver/nvidia/gpus` 是驱动真正探测到显卡之后才建目录的地方，
/// 探测不到就是空目录。用「目录里有没有子项」判断才准。
#[cfg(target_os = "linux")]
const DRIVER_USABLE_PROBES: &[(&str, &str)] = &[("nvidia_icd.json", "/proc/driver/nvidia/gpus")];

/// Vulkan 加载器默认读取的 ICD 配置目录。
#[cfg(target_os = "linux")]
const ICD_DIR: &str = "/usr/share/vulkan/icd.d";

/// 让加载器跳过指定驱动的环境变量。
#[cfg(target_os = "linux")]
const DISABLE_VAR: &str = "VK_LOADER_DRIVERS_DISABLE";

/// 应用启动时调用，返回这次被跳过的 ICD 名字。
///
/// 必须在创建 GPUI 之前调用：Vulkan 加载器在创建实例时读这个环境变量。
pub fn apply() -> Vec<String> {
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
    #[cfg(target_os = "linux")]
    {
        let skipped = unavailable_icds(
            |icd| std::path::Path::new(ICD_DIR).join(icd).is_file(),
            |probe| has_entries(std::path::Path::new(probe)),
        );
        if skipped.is_empty() {
            return skipped;
        }
        let merged = merge_disabled(std::env::var(DISABLE_VAR).ok(), skipped.iter().cloned());
        // 安全前提：只在 `main` 的最开头调用，此时进程还是单线程，
        // 没有其它线程在读环境变量。
        unsafe {
            std::env::set_var(DISABLE_VAR, merged.join(","));
        }
        log::info!("Vulkan 跳过用不了的驱动：{}", skipped.join(", "));
        skipped
    }
}

/// 目录里至少有一个子项。
///
/// 目录不存在、没有读权限、或者存在但是空的，都算「没有」。
#[cfg(target_os = "linux")]
fn has_entries(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

/// 挑出「配置文件在、驱动却用不了」的 ICD 名字。
///
/// 配置文件不在说明系统根本没装这个驱动，不需要处理。
#[cfg(target_os = "linux")]
fn unavailable_icds(
    icd_present: impl Fn(&str) -> bool,
    driver_usable: impl Fn(&str) -> bool,
) -> Vec<String> {
    DRIVER_USABLE_PROBES
        .iter()
        .filter(|(icd, probe)| icd_present(icd) && !driver_usable(probe))
        .map(|(icd, _)| (*icd).to_string())
        .collect()
}

/// 把这次要跳过的名字合并进已有的变量值：保留用户已经写的，去重，追加在后面。
#[cfg(target_os = "linux")]
fn merge_disabled(
    existing: Option<String>,
    extra: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut merged: Vec<String> = existing
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    for item in extra {
        if !merged.contains(&item) {
            merged.push(item);
        }
    }
    merged
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_driver_that_did_not_find_its_card_is_skipped() {
        let skipped = unavailable_icds(|icd| icd == "nvidia_icd.json", |_| false);
        assert_eq!(skipped, vec!["nvidia_icd.json".to_string()]);
    }

    #[test]
    fn a_driver_that_found_its_card_is_left_alone() {
        let skipped = unavailable_icds(|_| true, |_| true);
        assert!(skipped.is_empty());
    }

    #[test]
    fn an_empty_directory_counts_as_not_usable() {
        let dir = std::env::temp_dir().join("gistpic-vk-empty-dir");
        std::fs::create_dir_all(&dir).expect("建临时目录");
        assert!(!has_entries(&dir), "空目录不该算驱动可用");
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_missing_directory_counts_as_not_usable() {
        let dir = std::env::temp_dir().join("gistpic-vk-missing-dir");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!has_entries(&dir), "目录不存在不该算驱动可用");
    }

    #[test]
    fn a_driver_that_is_not_installed_is_left_alone() {
        let skipped = unavailable_icds(|_| false, |_| false);
        assert!(skipped.is_empty());
    }

    #[test]
    fn merging_keeps_what_the_user_already_set() {
        let merged = merge_disabled(
            Some("intel_icd.json, nvidia_icd.json".to_string()),
            vec!["nvidia_icd.json".to_string()],
        );
        assert_eq!(merged, vec!["intel_icd.json", "nvidia_icd.json"]);
    }

    #[test]
    fn merging_appends_new_entries() {
        let merged = merge_disabled(None, vec!["nvidia_icd.json".to_string()]);
        assert_eq!(merged, vec!["nvidia_icd.json"]);
    }

    #[test]
    fn merging_ignores_an_empty_setting() {
        let merged = merge_disabled(Some("  ".to_string()), vec!["nvidia_icd.json".to_string()]);
        assert_eq!(merged, vec!["nvidia_icd.json"]);
    }
}
