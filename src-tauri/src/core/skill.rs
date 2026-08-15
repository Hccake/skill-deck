// Skill 解析逻辑

use serde::{Deserialize, Serialize};
use specta::Type;
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use super::agent_availability::{
    availability_for_resolved_scope, resolved_agent_presence, AgentAvailabilityKind,
};
#[cfg(test)]
use super::agent_definition::AgentAdapter;
use super::agent_definition::AgentId;
#[cfg(test)]
use super::local_lock::read_local_lock;
use super::local_lock::LocalSkillLockEntry;
#[cfg(test)]
use super::paths::canonical_skills_dir;
#[cfg(test)]
use super::skill_lock::get_skill_from_lock;
use super::skill_lock::SkillLockEntry;
use super::skill_paths::find_skill_md_case_insensitive;
use super::update_metadata::{
    derive_update_capability, normalize_global_lock_entry, normalize_local_lock_entry,
};
#[cfg(test)]
use crate::environment::agent_environment::{AgentRuntimeSnapshot, DetectionState};
use crate::error::AppError;
#[cfg(test)]
use crate::models::AgentSkillPresence;
#[cfg(test)]
use std::collections::{BTreeSet, HashMap};

/// Skill 元数据
/// 对应 CLI: Skill (types.ts:42-49)
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMetadata {
    #[serde(default)]
    pub internal: bool,
}

/// SKILL.md frontmatter 结构
/// 对应 CLI: parseSkillMd 返回的数据结构
#[derive(Debug, Clone, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub metadata: Option<SkillMetadata>,
}

/// 解析 Native filesystem 中的 SKILL.md。
#[cfg(test)]
pub fn parse_skill_md(path: &Path) -> Result<SkillFrontmatter, AppError> {
    let content = std::fs::read_to_string(path)?;
    parse_skill_md_content(&content)
}

/// 解析已经由所属 Environment 读取的 SKILL.md 内容。
///
/// Discovery selector 通过该入口共享 Native 与 WSL 的 frontmatter 语义，
/// filesystem 访问仍由各自的 inventory collector 负责。
pub fn parse_skill_md_content(content: &str) -> Result<SkillFrontmatter, AppError> {
    // 检查是否以 --- 开头
    if !content.starts_with("---") {
        return Err(AppError::InvalidSkillMd {
            message: "Missing frontmatter delimiter".to_string(),
        });
    }

    // 找到第二个 ---
    let rest = &content[3..];
    let end_pos = rest.find("---").ok_or_else(|| AppError::InvalidSkillMd {
        message: "Unclosed frontmatter delimiter".to_string(),
    })?;

    // 提取 YAML 部分（跳过开头的换行符）
    let yaml_content = rest[..end_pos].trim();

    // 解析 YAML
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_content)?;

    // 验证必填字段
    if frontmatter.name.is_empty() {
        return Err(AppError::InvalidSkillMd {
            message: "Missing name field".to_string(),
        });
    }
    if frontmatter.description.is_empty() {
        return Err(AppError::InvalidSkillMd {
            message: "Missing description field".to_string(),
        });
    }

    Ok(frontmatter)
}

/// Sanitize skill 名称
/// 对应 CLI: sanitizeName (installer.ts:39-54)
pub fn sanitize_name(name: &str) -> String {
    let sanitized: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // 合并连续的 -
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // 去除首尾的 . 和 -
    let trimmed = result.trim_matches(|c| c == '.' || c == '-');

    if trimmed.is_empty() {
        "unnamed-skill".to_string()
    } else {
        trimmed.chars().take(255).collect()
    }
}

/// 已安装 Skill 的位置类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[specta(rename_all = "lowercase")]
pub enum InstalledSkillLocation {
    Global,
    Project,
}

/// 已安装的 Skill 信息
/// 对应 CLI: InstalledSkill (installer.ts:783-790)
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub canonical_path: String,
    pub scope: InstalledSkillLocation,
    pub agents: Vec<AgentId>,
    /// 每次读取 Skill 时根据当前 runtime 和文件系统重新组装，不写入 skill-lock。
    /// 只包含当前已检测到并且实际能够读取该 Skill 的关联 Agent。
    pub associated_agents: Vec<AgentId>,
    // 来自 skill-lock.json 的元数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_update: Option<bool>,
    /// 是否可直接执行更新
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_run_update: Option<bool>,
    /// 是否可自动检查更新
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_check_for_updates: Option<bool>,
    /// 更新能力缺失原因
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_reason: Option<String>,
    /// 所属 plugin 名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    /// Git ref（branch/tag）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// 来源仓库内的 Skill 相对路径，用于更新检查会话身份指纹
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// 默认可用 Agent 数量
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_available_agent_count: Option<u32>,
    /// 已独立适配 Agent 数量
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_adapted_agent_count: Option<u32>,
    /// 可清理的额外 Agent Skill 安装项数量（可能是链接或副本）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_copy_count: Option<u32>,
    /// 默认可用 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_available_agents: Option<Vec<AgentId>>,
    /// 需要/已有单独适配的 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_adapted_agents: Option<Vec<AgentId>>,
    /// 当前存在额外 Agent Skill 安装项的 Agents（可能是链接或副本）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_copy_agents: Option<Vec<AgentId>>,
    /// 当前只通过 Agent Skill 安装项使用的 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_only_agents: Option<Vec<AgentId>>,
    /// 需要额外保留到 Agent 专用 Skill 目录的默认可用 Agents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_copy_agents: Option<Vec<AgentId>>,
}

impl InstalledSkill {
    /// 从 global lock entry 填充元数据
    pub fn with_lock_entry(mut self, entry: Option<&SkillLockEntry>) -> Self {
        if let Some(e) = entry {
            self.source = Some(e.source.clone());
            self.source_url = normalize_global_lock_entry(e).source_url;
            self.installed_at = Some(e.installed_at.clone());
            self.updated_at = Some(e.updated_at.clone());
            self.plugin_name = e.plugin_name.clone();
            self.git_ref = e.ref_name.clone();
            self.skill_path = e.skill_path.clone();

            let capability = derive_update_capability(&normalize_global_lock_entry(e));
            self.can_run_update = Some(capability.can_run_update);
            self.can_check_for_updates = Some(capability.can_check_for_updates);
            self.update_reason = capability.reason;
        }
        self
    }

    /// 从 local lock entry 填充元数据（项目级）
    pub fn with_local_lock_entry(mut self, entry: Option<&LocalSkillLockEntry>) -> Self {
        if let Some(e) = entry {
            let metadata = normalize_local_lock_entry(e);
            self.source = Some(e.source.clone());
            self.source_url = metadata.source_url.clone();
            self.plugin_name = e.plugin_name.clone();
            self.git_ref = e.ref_name.clone();
            self.skill_path = e.skill_path.clone();

            let capability = derive_update_capability(&metadata);
            self.can_run_update = Some(capability.can_run_update);
            self.can_check_for_updates = Some(capability.can_check_for_updates);
            self.update_reason = capability.reason;
        }
        self
    }
}

#[cfg(test)]
fn apply_presence_summary(skill: &mut InstalledSkill, runtime: &AgentRuntimeSnapshot) {
    let is_global = matches!(skill.scope, InstalledSkillLocation::Global);
    let original_agents = skill.agents.clone();
    let mut effective_agents = Vec::new();
    let mut default_available_agents = Vec::new();
    let mut private_adapted_agents = Vec::new();
    let mut duplicate_copy_agents = Vec::new();
    let mut private_only_agents = Vec::new();
    let mut private_copy_agents = Vec::new();
    let mut default_available_count = 0usize;
    let mut private_adapted_count = 0usize;
    let mut duplicate_copy_count = 0usize;

    for (agent, resolved) in &runtime.agents {
        let presence = resolved_agent_presence(agent, resolved, &skill.name, is_global);
        let resolved_scope = if is_global {
            &resolved.global
        } else {
            &resolved.project
        };
        let availability = availability_for_resolved_scope(resolved_scope);
        match presence.presence {
            AgentSkillPresence::DefaultActive => {
                default_available_count += 1;
                default_available_agents.push(agent.clone());
                effective_agents.push(agent.clone());
            }
            AgentSkillPresence::DuplicateCopy => {
                default_available_count += 1;
                duplicate_copy_count += 1;
                default_available_agents.push(agent.clone());
                duplicate_copy_agents.push(agent.clone());
                private_copy_agents.push(agent.clone());
                effective_agents.push(agent.clone());
            }
            AgentSkillPresence::PrivateOnly => {
                private_only_agents.push(agent.clone());
                if availability.kind == AgentAvailabilityKind::StandardCompatible {
                    private_copy_agents.push(agent.clone());
                } else {
                    private_adapted_count += 1;
                    private_adapted_agents.push(agent.clone());
                }
                effective_agents.push(agent.clone());
            }
            AgentSkillPresence::RequiresPrivateInstall | AgentSkillPresence::NotInstalled => {
                if original_agents.contains(agent) {
                    private_adapted_count += 1;
                    private_adapted_agents.push(agent.clone());
                    effective_agents.push(agent.clone());
                }
            }
        }
    }

    let associated_agents = effective_agents
        .iter()
        .filter(|agent| {
            runtime
                .agents
                .get(*agent)
                .is_some_and(|resolved| resolved.detection == DetectionState::Detected)
        })
        .cloned()
        .collect();

    skill.agents = effective_agents;
    skill.associated_agents = associated_agents;
    skill.default_available_agent_count = Some(default_available_count as u32);
    skill.private_adapted_agent_count = Some(private_adapted_count as u32);
    skill.duplicate_copy_count = Some(duplicate_copy_count as u32);
    skill.default_available_agents = Some(default_available_agents);
    skill.private_adapted_agents = Some(private_adapted_agents);
    skill.duplicate_copy_agents = Some(duplicate_copy_agents);
    skill.private_only_agents = Some(private_only_agents);
    skill.private_copy_agents = Some(private_copy_agents);
}

/// 扫描目录信息
#[cfg(test)]
struct ScanScope {
    global: bool,
    path: std::path::PathBuf,
    path_identity: String,
    canonical: bool,
    agent_ids: BTreeSet<AgentId>,
}

#[cfg(test)]
fn add_scan_scope_owner(
    scopes: &mut Vec<ScanScope>,
    global: bool,
    path: std::path::PathBuf,
    path_identity: String,
    agent_id: AgentId,
) {
    if let Some(scope) = scopes
        .iter_mut()
        .find(|scope| scope.path_identity == path_identity && scope.global == global)
    {
        scope.agent_ids.insert(agent_id);
    } else {
        scopes.push(ScanScope {
            global,
            path,
            path_identity,
            canonical: false,
            agent_ids: BTreeSet::from([agent_id]),
        });
    }
}

/// 列出已安装的 skills
/// 对应 CLI: listInstalledSkills (installer.ts:797-1007)
#[cfg(test)]
pub fn list_installed_skills(
    scope: Option<InstalledSkillLocation>,
    cwd: &str,
    runtime: &AgentRuntimeSnapshot,
) -> Result<Vec<InstalledSkill>, AppError> {
    let mut skills_map: HashMap<String, InstalledSkill> = HashMap::new();
    let mut scopes: Vec<ScanScope> = Vec::new();

    // 预读项目级 local lock（如果存在）
    let local_lock = read_local_lock(cwd).ok();

    // 确定要扫描的 scope 类型
    let scope_types: Vec<bool> = match scope {
        Some(InstalledSkillLocation::Global) => vec![true],
        Some(InstalledSkillLocation::Project) => vec![false],
        None => vec![false, true], // 默认扫描 project 和 global
    };

    // 构建扫描目录列表
    // 对应 CLI: installer.ts 第 843-859 行
    for is_global in &scope_types {
        // 添加 canonical 目录
        let canonical_path = canonical_skills_dir(*is_global, cwd);
        scopes.push(ScanScope {
            global: *is_global,
            path_identity: canonical_path.to_string_lossy().to_string(),
            path: canonical_path,
            canonical: true,
            agent_ids: BTreeSet::new(),
        });

        // Runtime definitions are authoritative for current-scope private read paths.
        for (agent_id, resolved) in &runtime.agents {
            let resolved_scope = if *is_global {
                &resolved.global
            } else {
                &resolved.project
            };
            if !resolved_scope.enabled {
                continue;
            }
            let Some(private_path) = &resolved_scope.private_path else {
                continue;
            };
            let agent_dir = std::path::PathBuf::from(private_path);

            add_scan_scope_owner(
                &mut scopes,
                *is_global,
                agent_dir,
                private_path.clone(),
                agent_id.clone(),
            );
        }

        if !*is_global && crate::core::eve::is_eve_project(cwd) {
            let eve_id = runtime
                .agents
                .iter()
                .find(|(_, resolved)| resolved.definition.adapter == AgentAdapter::Eve)
                .map(|(id, _)| id.clone());
            let Some(eve_id) = eve_id else {
                continue;
            };
            let root = crate::core::eve::eve_root_skills_dir(cwd);
            if root.exists() {
                let root_identity = root.to_string_lossy().to_string();
                add_scan_scope_owner(&mut scopes, false, root, root_identity, eve_id.clone());
            }

            for subagent in crate::core::eve::list_eve_subagents(cwd) {
                let path = crate::core::eve::eve_subagent_skills_dir(cwd, &subagent);
                if path.exists() {
                    let path_identity = path.to_string_lossy().to_string();
                    add_scan_scope_owner(&mut scopes, false, path, path_identity, eve_id.clone());
                }
            }
        }
    }

    // 遍历每个扫描目录
    // 对应 CLI: installer.ts 第 861-1004 行
    for scope_info in &scopes {
        let entries = match std::fs::read_dir(&scope_info.path) {
            Ok(e) => e,
            Err(_) => continue, // 目录不存在，跳过
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // 只处理目录
            if !path.is_dir() {
                continue;
            }

            let Some(skill_md_path) = find_skill_md_case_insensitive(&path) else {
                continue;
            };

            // 解析 SKILL.md
            let frontmatter = match parse_skill_md(&skill_md_path) {
                Ok(f) => f,
                Err(_) => continue,
            };

            // 跳过 internal skills
            if frontmatter
                .metadata
                .as_ref()
                .map(|m| m.internal)
                .unwrap_or(false)
            {
                continue;
            }

            let scope_key = if scope_info.global {
                "global"
            } else {
                "project"
            };
            let skill_key = format!("{}:{}", scope_key, frontmatter.name);

            // Agent-specific roots may be shared by multiple runtime owners.
            if !scope_info.canonical {
                if let Some(existing) = skills_map.get_mut(&skill_key) {
                    for agent_id in &scope_info.agent_ids {
                        if !existing.agents.contains(agent_id) {
                            existing.agents.push(agent_id.clone());
                        }
                    }
                } else {
                    let skill = InstalledSkill {
                        name: frontmatter.name.clone(),
                        description: frontmatter.description,
                        path: path.to_string_lossy().to_string(),
                        canonical_path: path.to_string_lossy().to_string(),
                        scope: if scope_info.global {
                            InstalledSkillLocation::Global
                        } else {
                            InstalledSkillLocation::Project
                        },
                        agents: scope_info.agent_ids.iter().cloned().collect(),
                        associated_agents: Vec::new(),
                        source: None,
                        source_url: None,
                        installed_at: None,
                        updated_at: None,
                        has_update: None,
                        can_run_update: None,
                        can_check_for_updates: None,
                        update_reason: None,
                        plugin_name: None,
                        git_ref: None,
                        skill_path: None,
                        default_available_agent_count: None,
                        private_adapted_agent_count: None,
                        duplicate_copy_count: None,
                        default_available_agents: None,
                        private_adapted_agents: None,
                        duplicate_copy_agents: None,
                        private_only_agents: None,
                        private_copy_agents: None,
                    };

                    // 根据 scope 从对应的 lock 文件填充元数据
                    let skill = if scope_info.global {
                        let lock_entry = get_skill_from_lock(&frontmatter.name).ok().flatten();
                        skill.with_lock_entry(lock_entry.as_ref())
                    } else {
                        let local_entry = local_lock
                            .as_ref()
                            .and_then(|l| l.skills.get(&frontmatter.name));
                        skill.with_local_lock_entry(local_entry)
                    };

                    skills_map.insert(skill_key, skill);
                }
                continue;
            }

            // 对于 canonical 目录，检查哪些 agents 安装了这个 skill
            // 对应 CLI: installer.ts 第 911-980 行
            let sanitized_name = sanitize_name(&frontmatter.name);
            let dir_name = path.file_name().unwrap().to_string_lossy().to_string();
            let mut installed_agents: Vec<AgentId> = Vec::new();

            for (agent_id, resolved) in &runtime.agents {
                let resolved_scope = if scope_info.global {
                    &resolved.global
                } else {
                    &resolved.project
                };
                if !resolved_scope.enabled {
                    continue;
                }
                let Some(private_path) = &resolved_scope.private_path else {
                    continue;
                };
                let agent_base = std::path::PathBuf::from(private_path);

                // 尝试多种目录名匹配
                // 对应 CLI: installer.ts 第 925-947 行
                let possible_names: Vec<&str> = vec![&dir_name, &sanitized_name];

                let mut found = false;
                for possible_name in &possible_names {
                    let agent_skill_dir = agent_base.join(possible_name);
                    if agent_skill_dir.exists() {
                        found = true;
                        break;
                    }
                }

                // Fallback: 扫描目录并比对 SKILL.md 中的 name
                // 对应 CLI: installer.ts 第 951-975 行
                if !found {
                    if let Ok(agent_entries) = std::fs::read_dir(&agent_base) {
                        for agent_entry in agent_entries.filter_map(|e| e.ok()) {
                            let candidate_path = agent_entry.path();
                            if !candidate_path.is_dir() {
                                continue;
                            }

                            let Some(candidate_skill_md) =
                                find_skill_md_case_insensitive(&candidate_path)
                            else {
                                continue;
                            };
                            if let Ok(candidate_frontmatter) = parse_skill_md(&candidate_skill_md) {
                                if candidate_frontmatter.name == frontmatter.name {
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                }

                if found {
                    installed_agents.push(agent_id.clone());
                }
            }

            // 更新或插入 skill
            if let Some(existing) = skills_map.get_mut(&skill_key) {
                for agent in installed_agents {
                    if !existing.agents.contains(&agent) {
                        existing.agents.push(agent);
                    }
                }
            } else {
                let skill = InstalledSkill {
                    name: frontmatter.name.clone(),
                    description: frontmatter.description,
                    path: path.to_string_lossy().to_string(),
                    canonical_path: path.to_string_lossy().to_string(),
                    scope: if scope_info.global {
                        InstalledSkillLocation::Global
                    } else {
                        InstalledSkillLocation::Project
                    },
                    agents: installed_agents,
                    associated_agents: Vec::new(),
                    source: None,
                    source_url: None,
                    installed_at: None,
                    updated_at: None,
                    has_update: None,
                    can_run_update: None,
                    can_check_for_updates: None,
                    update_reason: None,
                    plugin_name: None,
                    git_ref: None,
                    skill_path: None,
                    default_available_agent_count: None,
                    private_adapted_agent_count: None,
                    duplicate_copy_count: None,
                    default_available_agents: None,
                    private_adapted_agents: None,
                    duplicate_copy_agents: None,
                    private_only_agents: None,
                    private_copy_agents: None,
                };

                // 根据 scope 从对应的 lock 文件填充元数据
                let skill = if scope_info.global {
                    let lock_entry = get_skill_from_lock(&frontmatter.name).ok().flatten();
                    skill.with_lock_entry(lock_entry.as_ref())
                } else {
                    let local_entry = local_lock
                        .as_ref()
                        .and_then(|l| l.skills.get(&frontmatter.name));
                    skill.with_local_lock_entry(local_entry)
                };

                skills_map.insert(skill_key, skill);
            }
        }
    }

    let mut skills: Vec<InstalledSkill> = skills_map.into_values().collect();
    for skill in &mut skills {
        apply_presence_summary(skill, runtime);
    }

    Ok(skills)
}

/// Read the markdown body of SKILL.md, stripping YAML frontmatter.
/// Takes the skill's canonical directory path.
pub fn read_skill_content(canonical_path: &str) -> Result<String, AppError> {
    let canonical_dir = std::path::Path::new(canonical_path);
    let skill_md = find_skill_md_case_insensitive(canonical_dir)
        .unwrap_or_else(|| canonical_dir.join("SKILL.md"));
    let content = std::fs::read_to_string(&skill_md).map_err(|_| AppError::PathNotFound {
        path: skill_md.to_string_lossy().to_string(),
    })?;

    Ok(skill_content_from_markdown(&content))
}

pub fn skill_content_from_markdown(content: &str) -> String {
    if let Some(stripped) = content.strip_prefix("---") {
        if let Some(end) = stripped.find("---") {
            let body_start = 3 + end + 3;
            return content[body_start..].trim_start_matches('\n').to_string();
        }
    }

    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent_definition::{
        AgentAdapter, AgentDefinition, AgentId, AgentSource, DetectionSpec, PathSpec,
        ScopeDefinition,
    };
    use crate::core::agent_registry::AgentRegistry;
    use crate::core::agents::AgentType;
    use crate::environment::agent_environment::{
        AgentEnvironmentResolver, AgentRuntimeSnapshot, DetectionState, DirectoryPresenceState,
        EnvironmentContext, ResolvedAgent, ResolvedAgentScope,
    };
    use crate::environment::types::{EnvironmentRef, EnvironmentStatus};
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    fn custom_runtime(project_path: &str, detection: DetectionState) -> AgentRuntimeSnapshot {
        let id = AgentId::parse("my-custom-agent").unwrap();
        let private_root = Path::new(project_path).join(".my-custom/skills");
        let resolved = ResolvedAgent {
            definition: AgentDefinition {
                id: id.clone(),
                display_name: "My Custom Agent".to_string(),
                source: AgentSource::Custom,
                aliases: Vec::new(),
                global: ScopeDefinition {
                    enabled: false,
                    reads_standard: false,
                    private_path: None,
                },
                project: ScopeDefinition {
                    enabled: true,
                    reads_standard: false,
                    private_path: Some(PathSpec::project(".my-custom/skills")),
                },
                detection: DetectionSpec::AnyPathExists {
                    paths: vec![PathSpec::project(".my-custom")],
                },
                legacy_paths: Vec::new(),
                adapter: AgentAdapter::Standard,
            },
            detection,
            detection_reason: None,
            global: ResolvedAgentScope {
                enabled: false,
                reads_standard: false,
                standard_path: None,
                private_path: None,
                read_paths: Vec::new(),
                standard_presence: None,
                private_presence: None,
                legacy_paths: Vec::new(),
            },
            project: ResolvedAgentScope {
                enabled: true,
                reads_standard: false,
                standard_path: Some(
                    Path::new(project_path)
                        .join(".agents/skills")
                        .to_string_lossy()
                        .to_string(),
                ),
                private_path: Some(private_root.to_string_lossy().to_string()),
                read_paths: vec![private_root.to_string_lossy().to_string()],
                standard_presence: Some(DirectoryPresenceState::Missing),
                private_presence: Some(DirectoryPresenceState::Present),
                legacy_paths: Vec::new(),
            },
        };
        AgentRuntimeSnapshot {
            registry_revision: "registry".to_string(),
            environment_revision: "environment".to_string(),
            environment: EnvironmentRef::Native,
            availability: EnvironmentStatus::Available,
            project_path: Some(project_path.to_string()),
            agents: BTreeMap::from([(id, resolved)]),
        }
    }

    fn builtin_runtime(project_path: &str) -> AgentRuntimeSnapshot {
        let registry = AgentRegistry::new(Vec::new());
        let resolver = AgentEnvironmentResolver::from_environment(EnvironmentContext {
            environment: EnvironmentRef::Native,
            home: project_path.to_string(),
            config_home: Path::new(project_path)
                .join(".config")
                .to_string_lossy()
                .to_string(),
            environment_variables: BTreeMap::new(),
            availability: EnvironmentStatus::Available,
            revision: "skill-tests-native".to_string(),
            wsl_workspace: None,
        });
        tauri::async_runtime::block_on(
            resolver.resolve_registry(registry.snapshot(), Some(project_path)),
        )
        .expect("resolve built-in runtime")
    }

    #[test]
    fn list_installed_skills_preserves_not_detected_custom_agent_id() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();
        let skill_dir = project.path().join(".my-custom/skills/demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        let runtime = custom_runtime(&cwd, DetectionState::NotDetected);

        let skills = list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime)
            .expect("list skills");
        let skill = skills.iter().find(|skill| skill.name == "demo").unwrap();
        let custom = AgentId::parse("my-custom-agent").unwrap();

        assert_eq!(skill.agents, vec![custom.clone()]);
        assert_eq!(skill.private_only_agents, Some(vec![custom.clone()]));
        assert_eq!(skill.private_adapted_agents, Some(vec![custom]));
        assert_eq!(skill.associated_agents, Vec::new());
    }

    #[test]
    fn installed_skill_serializes_associated_agents_as_the_public_contract() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();
        let skill_dir = project.path().join(".my-custom/skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();
        let runtime = custom_runtime(&cwd, DetectionState::Detected);
        let skill = list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime)
            .unwrap()
            .into_iter()
            .find(|skill| skill.name == "demo")
            .unwrap();

        let value = serde_json::to_value(skill).unwrap();
        assert_eq!(value["associatedAgents"][0], "my-custom-agent");
        assert!(value.get("cardAgents").is_none());
    }

    #[test]
    fn list_installed_skills_merges_custom_and_eve_subagent_owners() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();
        let subagent_root = project.path().join("agent/subagents/research/skills");
        let skill_dir = subagent_root.join("demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Shared private skill\n---\n",
        )
        .unwrap();
        fs::write(
            project.path().join("package.json"),
            r#"{"dependencies":{"eve":"^0.11.5"}}"#,
        )
        .unwrap();

        let mut runtime = builtin_runtime(&cwd);
        let eve = AgentId::parse("eve").unwrap();
        assert_eq!(runtime.agents[&eve].detection, DetectionState::Detected);

        let custom = AgentId::parse("my-custom-agent").unwrap();
        let mut custom_resolved = custom_runtime(&cwd, DetectionState::Detected)
            .agents
            .remove(&custom)
            .unwrap();
        let subagent_root = subagent_root.to_string_lossy().to_string();
        custom_resolved.project.private_path = Some(subagent_root.clone());
        custom_resolved.project.read_paths = vec![subagent_root];
        runtime.agents.insert(custom.clone(), custom_resolved);

        let skills =
            list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime).unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "demo")
            .expect("colliding Eve/Custom skill should be listed");

        assert_eq!(skill.agents, vec![eve.clone(), custom.clone()]);
        assert_eq!(skill.associated_agents, vec![eve, custom]);
    }

    #[test]
    fn list_installed_skills_merges_two_custom_owners_in_stable_order() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();
        let skill_dir = project.path().join(".my-custom/skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Shared private skill\n---\n",
        )
        .unwrap();

        let first = AgentId::parse("my-custom-agent").unwrap();
        let second = AgentId::parse("my-custom-agent-two").unwrap();
        let mut runtime = custom_runtime(&cwd, DetectionState::Detected);
        let mut second_resolved = runtime.agents[&first].clone();
        second_resolved.definition.id = second.clone();
        second_resolved.definition.display_name = "My Custom Agent Two".to_string();
        runtime.agents.insert(second.clone(), second_resolved);

        let skills =
            list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime).unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "demo")
            .expect("shared Custom skill should be listed");

        assert_eq!(skill.agents, vec![first.clone(), second.clone()]);
        assert_eq!(skill.associated_agents, vec![first, second]);
    }

    #[test]
    fn scan_scope_owner_merge_uses_exact_resolved_path_strings() {
        let first = AgentId::parse("my-custom-agent").unwrap();
        let second = AgentId::parse("my-custom-agent-two").unwrap();
        let third = AgentId::parse("my-custom-agent-three").unwrap();
        let mut scopes = Vec::new();

        add_scan_scope_owner(
            &mut scopes,
            false,
            Path::new("agent/skills").to_path_buf(),
            "agent/skills".to_string(),
            first.clone(),
        );
        add_scan_scope_owner(
            &mut scopes,
            false,
            Path::new("agent/./skills").to_path_buf(),
            "agent/./skills".to_string(),
            second.clone(),
        );
        add_scan_scope_owner(
            &mut scopes,
            false,
            Path::new("agent/skills").to_path_buf(),
            "agent/skills".to_string(),
            third.clone(),
        );

        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].agent_ids, BTreeSet::from([first, third]));
        assert_eq!(scopes[1].agent_ids, BTreeSet::from([second]));
    }

    #[test]
    fn test_parse_valid_skill_md() {
        let content = r#"---
name: test-skill
description: A test skill
---

# Test Skill

Content here.
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let result = parse_skill_md(file.path()).unwrap();
        assert_eq!(result.name, "test-skill");
        assert_eq!(result.description, "A test skill");
    }

    #[test]
    fn test_parse_skill_md_with_metadata() {
        let content = r#"---
name: internal-skill
description: An internal skill
metadata:
  internal: true
---

Content.
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let result = parse_skill_md(file.path()).unwrap();
        assert_eq!(result.name, "internal-skill");
        assert!(result.metadata.unwrap().internal);
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# No frontmatter\n\nJust content.";
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let result = parse_skill_md(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn skill_content_from_markdown_strips_frontmatter() {
        let content = "---\nname: toolkit\ndescription: Toolkit\n---\n\n# Toolkit\n";

        assert_eq!(skill_content_from_markdown(content), "# Toolkit\n");
        assert_eq!(skill_content_from_markdown("# Plain\n"), "# Plain\n");
    }

    #[test]
    fn test_sanitize_name_basic() {
        assert_eq!(sanitize_name("Hello World"), "hello-world");
        assert_eq!(sanitize_name("my_skill.v2"), "my_skill.v2");
        assert_eq!(sanitize_name("---test---"), "test");
        assert_eq!(sanitize_name("UPPERCASE"), "uppercase");
    }

    #[test]
    fn test_sanitize_name_special_chars() {
        assert_eq!(sanitize_name("skill@v1!"), "skill-v1");
        assert_eq!(sanitize_name("../path/traversal"), "path-traversal");
        assert_eq!(sanitize_name(""), "unnamed-skill");
    }

    #[test]
    fn test_sanitize_name_consecutive_hyphens() {
        assert_eq!(sanitize_name("a  b  c"), "a-b-c");
        assert_eq!(sanitize_name("a---b"), "a-b");
    }

    #[test]
    fn test_list_installed_skills_scans_existing_dirs_for_undetected_agents() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();

        let detected = AgentType::detect_installed();
        let undetected_agent = AgentType::all()
            .find(|agent| {
                !detected.contains(agent) && agent.config().skills_dir != ".agents/skills"
            })
            .expect("expected at least one undetected agent with a separate skill directory");

        let agent_dir = project
            .path()
            .join(undetected_agent.config().skills_dir)
            .join("ghost-skill");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(
            agent_dir.join("SKILL.md"),
            "---\nname: ghost-skill\ndescription: Hidden skill\n---\n",
        )
        .unwrap();

        let mut runtime = builtin_runtime(&cwd);
        let undetected_id = AgentId::parse(undetected_agent.to_string()).unwrap();
        runtime.agents.get_mut(&undetected_id).unwrap().detection = DetectionState::NotDetected;
        let skills =
            list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime).unwrap();

        let ghost_skill = skills
            .iter()
            .find(|skill| skill.name == "ghost-skill")
            .expect("skill should be visible even when agent is undetected");
        assert!(
            ghost_skill.agents.contains(&undetected_id),
            "skill should be associated with the undetected agent directory"
        );
        assert!(
            !ghost_skill.associated_agents.contains(&undetected_id),
            "associated agents should exclude undetected agents"
        );
    }

    #[test]
    fn test_presence_summary_associated_agents_exclude_undetected_agents() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();

        let detected = AgentType::detect_installed();
        let undetected_agent = AgentType::all()
            .find(|agent| {
                !detected.contains(agent) && agent.config().skills_dir != ".agents/skills"
            })
            .expect("expected at least one undetected agent with a separate skill directory");

        let agent_dir = project
            .path()
            .join(undetected_agent.config().skills_dir)
            .join("ghost-skill");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(
            agent_dir.join("SKILL.md"),
            "---\nname: ghost-skill\ndescription: Hidden skill\n---\n",
        )
        .unwrap();

        let undetected_id = AgentId::parse(undetected_agent.to_string()).unwrap();
        let mut runtime = builtin_runtime(&cwd);
        runtime.agents.get_mut(&undetected_id).unwrap().detection = DetectionState::NotDetected;
        let mut skill = InstalledSkill {
            name: "ghost-skill".to_string(),
            description: "Hidden skill".to_string(),
            path: agent_dir.to_string_lossy().to_string(),
            canonical_path: agent_dir.to_string_lossy().to_string(),
            scope: InstalledSkillLocation::Project,
            agents: vec![undetected_id.clone()],
            associated_agents: Vec::new(),
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: None,
            can_run_update: None,
            can_check_for_updates: None,
            update_reason: None,
            plugin_name: None,
            git_ref: None,
            skill_path: None,
            default_available_agent_count: None,
            private_adapted_agent_count: None,
            duplicate_copy_count: None,
            default_available_agents: None,
            private_adapted_agents: None,
            duplicate_copy_agents: None,
            private_only_agents: None,
            private_copy_agents: None,
        };

        apply_presence_summary(&mut skill, &runtime);

        assert!(
            skill.agents.contains(&undetected_id),
            "presence summary should keep undetected private agent effective"
        );
        assert!(
            !skill.associated_agents.contains(&undetected_id),
            "associated agents should exclude undetected agents"
        );
    }

    #[test]
    fn test_list_installed_skills_preserves_frontmatter_fallback_agents_after_presence_summary() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();

        let agent = AgentType::ClaudeCode;
        let agent_dir = project
            .path()
            .join(agent.config().skills_dir)
            .join("custom-folder");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(
            agent_dir.join("SKILL.md"),
            "---\nname: legacy-name\ndescription: Legacy folder\n---\n",
        )
        .unwrap();

        let runtime = builtin_runtime(&cwd);
        let skills =
            list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime).unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "legacy-name")
            .expect("legacy skill should be visible via frontmatter fallback");

        assert!(
            skill
                .agents
                .contains(&AgentId::parse(agent.to_string()).unwrap()),
            "presence summary should preserve agents found by frontmatter fallback"
        );
        assert!(skill.private_adapted_agent_count.unwrap_or_default() > 0);
    }

    #[test]
    fn test_list_installed_skills_scans_eve_subagent_targets() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();
        fs::create_dir_all(project.path().join("agent/subagents/research/skills/demo")).unwrap();
        fs::write(
            project
                .path()
                .join("agent/subagents/research/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Eve subagent skill\n---\n",
        )
        .unwrap();
        fs::write(
            project.path().join("package.json"),
            r#"{"dependencies":{"eve":"^0.11.5"}}"#,
        )
        .unwrap();

        let runtime = builtin_runtime(&cwd);
        let skills =
            list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime).unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "demo")
            .expect("Eve subagent skill should be listed");

        let eve = AgentId::parse("eve").unwrap();
        assert!(skill.agents.contains(&eve));
        assert!(
            skill.associated_agents.contains(&eve),
            "project-aware Eve targets should be visible on skill cards"
        );
    }

    #[test]
    fn test_list_installed_skills_adds_presence_summary_counts() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();

        let standard_dir = project.path().join(".agents").join("skills").join("demo");
        fs::create_dir_all(&standard_dir).unwrap();
        fs::write(
            standard_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();

        let private_dir = project.path().join(".claude").join("skills").join("demo");
        fs::create_dir_all(&private_dir).unwrap();
        fs::write(
            private_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n",
        )
        .unwrap();

        let runtime = builtin_runtime(&cwd);
        let skills =
            list_installed_skills(Some(InstalledSkillLocation::Project), &cwd, &runtime).unwrap();
        let skill = skills
            .iter()
            .find(|skill| skill.name == "demo")
            .expect("demo skill");

        assert!(skill.default_available_agent_count.unwrap_or_default() > 0);
        assert!(skill.private_adapted_agent_count.unwrap_or_default() > 0);
        assert_eq!(skill.duplicate_copy_count, Some(0));
        assert!(skill.agents.contains(&AgentId::parse("codex").unwrap()));
        assert!(skill
            .agents
            .contains(&AgentId::parse("claude-code").unwrap()));
    }

    #[test]
    fn test_with_local_lock_entry_prefers_explicit_source_url() {
        let project = tempdir().unwrap();
        let cwd = project.path().to_string_lossy().to_string();
        fs::write(
            project.path().join("skills-lock.json"),
            r#"{
  "version": 1,
  "skills": {
    "ssh-skill": {
      "source": "owner/private-repo",
      "sourceType": "github",
      "sourceUrl": "git@github.com:owner/private-repo.git",
      "computedHash": "abc123"
    }
  }
}
"#,
        )
        .unwrap();

        let local_lock = read_local_lock(&cwd).unwrap();
        let entry = local_lock.skills.get("ssh-skill").unwrap();
        let skill = InstalledSkill {
            name: "ssh-skill".to_string(),
            description: "SSH skill".to_string(),
            path: String::new(),
            canonical_path: String::new(),
            scope: InstalledSkillLocation::Project,
            agents: Vec::new(),
            associated_agents: Vec::new(),
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: None,
            can_run_update: None,
            can_check_for_updates: None,
            update_reason: None,
            plugin_name: None,
            git_ref: None,
            skill_path: None,
            default_available_agent_count: None,
            private_adapted_agent_count: None,
            duplicate_copy_count: None,
            default_available_agents: None,
            private_adapted_agents: None,
            duplicate_copy_agents: None,
            private_only_agents: None,
            private_copy_agents: None,
        }
        .with_local_lock_entry(Some(entry));

        assert_eq!(
            skill.source_url.as_deref(),
            Some("git@github.com:owner/private-repo.git")
        );
    }

    #[test]
    fn test_with_local_lock_entry_does_not_invent_source_url_for_empty_source() {
        let entry = LocalSkillLockEntry {
            source: String::new(),
            ref_name: None,
            source_type: "github".to_string(),
            source_url: None,
            well_known_digest: None,
            computed_hash: String::new(),
            remote_hash: Some("tree123".to_string()),
            skill_path: Some("skills/demo/SKILL.md".to_string()),
            subagents: None,
            plugin_name: None,
        };

        let skill = InstalledSkill {
            name: "demo".to_string(),
            description: "Demo".to_string(),
            path: String::new(),
            canonical_path: String::new(),
            scope: InstalledSkillLocation::Project,
            agents: Vec::new(),
            associated_agents: Vec::new(),
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: None,
            can_run_update: None,
            can_check_for_updates: None,
            update_reason: None,
            plugin_name: None,
            git_ref: None,
            skill_path: None,
            default_available_agent_count: None,
            private_adapted_agent_count: None,
            duplicate_copy_count: None,
            default_available_agents: None,
            private_adapted_agents: None,
            duplicate_copy_agents: None,
            private_only_agents: None,
            private_copy_agents: None,
        }
        .with_local_lock_entry(Some(&entry));

        assert_eq!(skill.source_url, None);
        assert_eq!(skill.skill_path.as_deref(), Some("skills/demo/SKILL.md"));
    }

    #[test]
    fn test_installed_skill_runtime_update_capabilities_can_be_stored() {
        let skill = InstalledSkill {
            name: "demo".to_string(),
            description: "Demo".to_string(),
            path: String::new(),
            canonical_path: String::new(),
            scope: InstalledSkillLocation::Global,
            agents: Vec::new(),
            associated_agents: Vec::new(),
            source: None,
            source_url: None,
            installed_at: None,
            updated_at: None,
            has_update: Some(false),
            can_run_update: Some(true),
            can_check_for_updates: Some(false),
            update_reason: Some("missing-skill-path".to_string()),
            plugin_name: None,
            git_ref: None,
            skill_path: None,
            default_available_agent_count: None,
            private_adapted_agent_count: None,
            duplicate_copy_count: None,
            default_available_agents: None,
            private_adapted_agents: None,
            duplicate_copy_agents: None,
            private_only_agents: None,
            private_copy_agents: None,
        };

        assert_eq!(skill.can_run_update, Some(true));
        assert_eq!(skill.can_check_for_updates, Some(false));
        assert_eq!(skill.update_reason.as_deref(), Some("missing-skill-path"));
    }

    #[test]
    fn test_read_skill_content_returns_body_without_frontmatter() {
        let content =
            "---\nname: test\ndescription: A test\n---\n\n# Test Skill\n\nBody content here.\n";
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("test-skill");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();

        let result = read_skill_content(&skill_dir.to_string_lossy()).unwrap();
        assert!(result.starts_with("# Test Skill"));
        assert!(result.contains("Body content here."));
        assert!(!result.contains("---"));
        assert!(!result.contains("name: test"));
    }

    #[test]
    fn test_read_skill_content_missing_skill_md() {
        let dir = tempdir().unwrap();
        let result = read_skill_content(&dir.path().to_string_lossy());
        assert!(result.is_err());
    }

    #[test]
    fn test_read_skill_content_no_frontmatter() {
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("plain");
        fs::create_dir(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "# Just content\n\nNo frontmatter.",
        )
        .unwrap();

        let result = read_skill_content(&skill_dir.to_string_lossy()).unwrap();
        assert!(result.starts_with("# Just content"));
    }
}
