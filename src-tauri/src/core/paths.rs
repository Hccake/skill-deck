// 路径工具函数
// 对应 CLI: agents.ts 顶层常量 (home, configHome)

use std::path::PathBuf;
use std::sync::LazyLock;

/// 路径上下文（与 CLI 顶层常量对应）
/// 使用 LazyLock 单例，只初始化一次
pub static PATHS: LazyLock<PathContext> = LazyLock::new(PathContext::new);

/// 路径上下文
/// 对应 CLI: agents.ts 第 7-11 行
pub struct PathContext {
    /// 用户主目录
    /// 对应 CLI: const home = homedir();
    pub home: PathBuf,

    /// XDG 配置目录
    /// 对应 CLI: const configHome = xdgConfig ?? join(home, '.config');
    pub config_home: PathBuf,
}

impl PathContext {
    fn new() -> Self {
        let home = dirs::home_dir().expect("Failed to get home directory");
        // 对齐 CLI: xdgConfig ?? join(home, '.config')
        // xdg-basedir 在 Windows/macOS 上返回 None，fallback 到 ~/.config
        // dirs::config_dir() 在 Windows 上返回 AppData\Roaming，与 CLI 不一致
        // 因此仅在 Linux 上使用 XDG_CONFIG_HOME，其余平台统一用 ~/.config
        let config_home = if cfg!(target_os = "linux") {
            std::env::var("XDG_CONFIG_HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
        } else {
            home.join(".config")
        };

        Self { home, config_home }
    }
}
