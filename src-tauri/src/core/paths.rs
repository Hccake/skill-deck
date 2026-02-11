// 路径工具函数
// 对应 CLI: agents.ts 顶层常量 (home, configHome, codexHome, claudeHome)

use once_cell::sync::Lazy;
use std::path::PathBuf;

/// 路径上下文（与 CLI 顶层常量对应）
/// 使用 Lazy 单例，只初始化一次
pub static PATHS: Lazy<PathContext> = Lazy::new(PathContext::new);

/// 路径上下文
/// 对应 CLI: agents.ts 第 7-11 行
pub struct PathContext {
    /// 用户主目录
    /// 对应 CLI: const home = homedir();
    pub home: PathBuf,

    /// XDG 配置目录
    /// 对应 CLI: const configHome = xdgConfig ?? join(home, '.config');
    pub config_home: PathBuf,

    /// Codex 主目录
    /// 对应 CLI: const codexHome = process.env.CODEX_HOME?.trim() || join(home, '.codex');
    pub codex_home: PathBuf,

    /// Claude 配置目录
    /// 对应 CLI: const claudeHome = process.env.CLAUDE_CONFIG_DIR?.trim() || join(home, '.claude');
    pub claude_home: PathBuf,
}

impl PathContext {
    fn new() -> Self {
        let home = dirs::home_dir().expect("Failed to get home directory");
        let config_home = dirs::config_dir().unwrap_or_else(|| home.join(".config"));

        let codex_home = std::env::var("CODEX_HOME")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));

        let claude_home = std::env::var("CLAUDE_CONFIG_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));

        Self {
            home,
            config_home,
            codex_home,
            claude_home,
        }
    }
}

/// 获取 canonical skills 目录
/// 对应 CLI: getCanonicalSkillsDir (installer.ts:74-77)
/// Global: ~/.agents/skills/
/// Project: ./.agents/skills/
pub fn canonical_skills_dir(global: bool, cwd: &str) -> PathBuf {
    let base = if global {
        PATHS.home.clone()
    } else {
        PathBuf::from(cwd)
    };
    base.join(".agents").join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_initialized() {
        // 验证 PATHS 单例可以正常初始化
        assert!(PATHS.home.exists(), "Home directory should exist");
        assert!(
            !PATHS.config_home.as_os_str().is_empty(),
            "Config home should not be empty"
        );
    }

    #[test]
    fn test_canonical_skills_dir_global() {
        let dir = canonical_skills_dir(true, "/some/project");
        let dir_str = dir.to_string_lossy();
        assert!(dir_str.contains(".agents"), "Should contain .agents");
        assert!(dir_str.contains("skills"), "Should contain skills");
        assert!(
            !dir_str.contains("some"),
            "Global mode should not contain project path"
        );
    }

    #[test]
    fn test_canonical_skills_dir_project() {
        let dir = canonical_skills_dir(false, "/some/project");
        let dir_str = dir.to_string_lossy();
        assert!(dir_str.contains(".agents"), "Should contain .agents");
        assert!(dir_str.contains("skills"), "Should contain skills");
    }
}
