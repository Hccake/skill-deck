use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::time::Duration;

use crate::core::agent_availability::AgentAvailabilityKind;
use crate::core::agents::AgentType;
use crate::core::skill::SkillFrontmatter;
use crate::environment::agent_environment::{
    AgentEnvironmentContext, AgentEnvironmentResolver, AgentEnvironmentTarget,
};
use crate::environment::types::{ContextRef, ContextScope, ProjectBinding, ResourceLocator};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{decode_nul_records, run_wsl_script};
use crate::error::AppError;
use crate::models::InstallTargetInfo;

const WSL_INSPECT_SCRIPT: &str = r#"
scan_root() {
  agent=$1
  root=$2
  [ -d "$root" ] || return 0
  for dir in "$root"/*; do
    [ -d "$dir" ] || continue
    skill_md=
    for candidate in "$dir"/*; do
      [ -f "$candidate" ] || continue
      base=${candidate##*/}
      lower=$(printf '%s' "$base" | tr '[:upper:]' '[:lower:]')
      if [ "$lower" = 'skill.md' ]; then
        skill_md=$candidate
        break
      fi
    done
    [ -n "$skill_md" ] || continue
    frontmatter=$(awk 'BEGIN { delimiters=0; data="" } /^---[[:space:]]*$/ { delimiters++; data=data $0 ORS; if (delimiters == 2) { printf "%s", data; exit } next } delimiters == 1 { data=data $0 ORS }' "$skill_md")
    [ -n "$frontmatter" ] || continue
    printf 'skill\0%s\0%s\0%s\0' "$agent" "$dir" "$frontmatter"
  done
}

printf '2\0'
context_root=$1
canonical=$2
shift 2
if [ -d "$context_root" ]; then printf '1\0'; else printf '0\0'; fi
scan_root - "$canonical"

while [ "$#" -ge 5 ]; do
  agent=$1
  root=$2
  d1=$3
  d2=$4
  d3=$5
  shift 5
  if [ "$agent" = eve ]; then
    if [ -d "$d1" ] && [ -f "$d2" ]; then
      printf 'eve-package\0'
      cat -- "$d2"
      printf '\0'
    else
      printf 'detected\0eve\00\0'
    fi
    [ -n "$root" ] && scan_root eve:root "$root"
    for subagent in "$d1/subagents"/*; do
      [ -d "$subagent" ] || continue
      scan_root "eve:${subagent##*/}" "$subagent/skills"
    done
  else
    detected=0
    if { [ -n "$d1" ] && [ -e "$d1" ]; } || { [ -n "$d2" ] && [ -e "$d2" ]; } || { [ -n "$d3" ] && [ -e "$d3" ]; }; then
      detected=1
    fi
    printf 'detected\0%s\0%s\0' "$agent" "$detected"
    [ -n "$root" ] && scan_root "$agent" "$root"
  fi
done
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    pub context: ContextRef,
    pub project: Option<ProjectBinding>,
    pub home: ResourceLocator,
    pub skill_root: ResourceLocator,
    pub lock: ResourceLocator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectRequest {
    pub context: ResolvedContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillEntrySnapshot {
    pub name: String,
    pub description: String,
    pub canonical_path: String,
    pub canonical_present: bool,
    pub agents: Vec<AgentType>,
    pub card_agents: Vec<AgentType>,
    pub default_available_agents: Vec<AgentType>,
    pub private_adapted_agents: Vec<AgentType>,
    pub duplicate_copy_agents: Vec<AgentType>,
    pub private_only_agents: Vec<AgentType>,
    pub private_copy_agents: Vec<AgentType>,
    pub eve_targets: Vec<InstallTargetInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub path_exists: bool,
    pub detected_agents: Vec<AgentType>,
    pub skills: Vec<SkillEntrySnapshot>,
}

pub enum EnvironmentService {
    Wsl(WslSession),
}

impl EnvironmentService {
    pub async fn inspect(&self, request: &InspectRequest) -> Result<EnvironmentSnapshot, AppError> {
        match self {
            Self::Wsl(session) => inspect_wsl_context(session, request).await,
        }
    }
}

async fn inspect_wsl_context(
    session: &WslSession,
    request: &InspectRequest,
) -> Result<EnvironmentSnapshot, AppError> {
    let is_global = matches!(request.context.context.scope, ContextScope::Global);
    let project_path = request
        .context
        .project
        .as_ref()
        .map(|project| project.native_path.as_str())
        .unwrap_or(session.home.as_str());
    let resolver = AgentEnvironmentResolver::new(AgentEnvironmentContext {
        home: session.home.clone(),
        config_home: session.config_home.clone(),
        env: session.environment.clone(),
    });
    let targets: Vec<_> = AgentType::all()
        .map(|agent| resolver.target(agent, is_global, project_path))
        .collect();
    let context_root = if is_global {
        session.home.clone()
    } else {
        project_path.to_string()
    };
    let mut args = vec![context_root, request.context.skill_root.native_path.clone()];
    for target in &targets {
        args.push(target.agent.to_string());
        args.push(target.private_path.clone().unwrap_or_default());
        for index in 0..3 {
            args.push(
                target
                    .detection_paths
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
    }
    let output = run_wsl_script(
        session,
        WSL_INSPECT_SCRIPT,
        &args,
        Vec::new(),
        Duration::from_secs(20),
    )
    .await?;
    parse_wsl_inspect_output(&output, &targets)
}

pub fn parse_wsl_inspect_output(
    bytes: &[u8],
    targets: &[AgentEnvironmentTarget],
) -> Result<EnvironmentSnapshot, AppError> {
    let records = decode_nul_records(bytes);
    if records.first().map(String::as_str) != Some("2")
        || !matches!(records.get(1).map(String::as_str), Some("0" | "1"))
    {
        return Err(AppError::Custom {
            message: "invalid WSL inspect response".to_string(),
        });
    }

    #[derive(Debug)]
    struct Candidate {
        description: String,
        canonical_path: String,
        canonical_present: bool,
        original_agents: Vec<AgentType>,
        eve_targets: Vec<InstallTargetInfo>,
    }

    let mut detected = HashMap::new();
    let mut candidates: HashMap<String, Candidate> = HashMap::new();
    let mut index = 2;
    while index < records.len() {
        match records[index].as_str() {
            "detected" if index + 2 < records.len() => {
                let agent = AgentType::from_str(&records[index + 1]).map_err(|message| {
                    AppError::Custom {
                        message: format!("invalid WSL inspect agent: {message}"),
                    }
                })?;
                let is_detected = match records[index + 2].as_str() {
                    "0" => false,
                    "1" => true,
                    _ => {
                        return Err(AppError::Custom {
                            message: "invalid WSL inspect detection flag".to_string(),
                        })
                    }
                };
                detected.insert(agent, is_detected);
                index += 3;
            }
            "skill" if index + 3 < records.len() => {
                let agent_token = records[index + 1].as_str();
                let (agent, eve_target) = if agent_token == "-" {
                    (None, None)
                } else if let Some(target_name) = agent_token.strip_prefix("eve:") {
                    let subagent = (target_name != "root").then(|| target_name.to_string());
                    (
                        Some(AgentType::Eve),
                        Some((agent_token.to_string(), subagent)),
                    )
                } else {
                    (
                        Some(AgentType::from_str(agent_token).map_err(|message| {
                            AppError::Custom {
                                message: format!("invalid WSL inspect skill agent: {message}"),
                            }
                        })?),
                        None,
                    )
                };
                let canonical_path = records[index + 2].clone();
                let is_eve_subagent = eve_target
                    .as_ref()
                    .is_some_and(|(_, subagent)| subagent.is_some());
                if let Some(frontmatter) = parse_skill_frontmatter(&records[index + 3]) {
                    if !frontmatter
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.internal)
                    {
                        let entry =
                            candidates
                                .entry(frontmatter.name.clone())
                                .or_insert_with(|| Candidate {
                                    description: frontmatter.description.clone(),
                                    canonical_path: canonical_path.clone(),
                                    canonical_present: agent.is_none(),
                                    original_agents: Vec::new(),
                                    eve_targets: Vec::new(),
                                });
                        if agent.is_none() {
                            entry.description = frontmatter.description;
                            entry.canonical_path = canonical_path.clone();
                            entry.canonical_present = true;
                        } else if let Some(agent) = agent {
                            // Eve subagents are fallback adapted targets; they do not mean
                            // the Eve root skill path exists.
                            if !is_eve_subagent && !entry.original_agents.contains(&agent) {
                                entry.original_agents.push(agent);
                            }
                        }
                        if let Some((target_id, subagent)) = eve_target {
                            if !entry
                                .eve_targets
                                .iter()
                                .any(|target| target.target_id == target_id)
                            {
                                entry.eve_targets.push(InstallTargetInfo {
                                    target_id,
                                    agent: AgentType::Eve,
                                    display_name: crate::core::eve::eve_target_label(
                                        subagent.as_deref(),
                                    ),
                                    subagent,
                                    path: canonical_path,
                                });
                            }
                        }
                    }
                }
                index += 4;
            }
            "eve-package" if index + 1 < records.len() => {
                let is_eve_project = serde_json::from_str::<serde_json::Value>(&records[index + 1])
                    .ok()
                    .is_some_and(|package| {
                        ["dependencies", "devDependencies"]
                            .into_iter()
                            .any(|section| {
                                package
                                    .get(section)
                                    .and_then(serde_json::Value::as_object)
                                    .is_some_and(|entries| entries.contains_key("eve"))
                            })
                    });
                detected.insert(AgentType::Eve, is_eve_project);
                index += 2;
            }
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL inspect record".to_string(),
                })
            }
        }
    }

    let mut skills = Vec::with_capacity(candidates.len());
    for (name, candidate) in candidates {
        let mut agents = Vec::new();
        let mut card_agents = Vec::new();
        let mut default_available_agents = Vec::new();
        let mut private_adapted_agents = Vec::new();
        let mut duplicate_copy_agents = Vec::new();
        let mut private_only_agents = Vec::new();
        let mut private_copy_agents = Vec::new();

        for target in targets {
            let shared_exists = candidate.canonical_present;
            let private_exists = candidate.original_agents.contains(&target.agent);
            let presence = if shared_exists && private_exists && target.default_available {
                Presence::DuplicateCopy
            } else if shared_exists && !private_exists && target.default_available {
                Presence::DefaultActive
            } else if private_exists {
                Presence::PrivateOnly
            } else if shared_exists && !target.default_available {
                Presence::RequiresPrivateInstall
            } else {
                Presence::NotInstalled
            };

            match presence {
                Presence::DefaultActive => {
                    default_available_agents.push(target.agent);
                    agents.push(target.agent);
                }
                Presence::DuplicateCopy => {
                    default_available_agents.push(target.agent);
                    duplicate_copy_agents.push(target.agent);
                    private_copy_agents.push(target.agent);
                    agents.push(target.agent);
                }
                Presence::PrivateOnly => {
                    private_only_agents.push(target.agent);
                    if target.availability == AgentAvailabilityKind::SharedCompatible {
                        private_copy_agents.push(target.agent);
                    } else {
                        private_adapted_agents.push(target.agent);
                    }
                    agents.push(target.agent);
                }
                Presence::RequiresPrivateInstall | Presence::NotInstalled => {}
            }

            if agents.contains(&target.agent)
                && detected.get(&target.agent).copied().unwrap_or(false)
            {
                card_agents.push(target.agent);
            }
        }

        if !candidate.eve_targets.is_empty() && !agents.contains(&AgentType::Eve) {
            agents.push(AgentType::Eve);
            private_adapted_agents.push(AgentType::Eve);
            if detected.get(&AgentType::Eve).copied().unwrap_or(false) {
                card_agents.push(AgentType::Eve);
            }
        }

        skills.push(SkillEntrySnapshot {
            name,
            description: candidate.description,
            canonical_path: candidate.canonical_path,
            canonical_present: candidate.canonical_present,
            agents,
            card_agents,
            default_available_agents,
            private_adapted_agents,
            duplicate_copy_agents,
            private_only_agents,
            private_copy_agents,
            eve_targets: candidate.eve_targets,
        });
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    let detected_agents = targets
        .iter()
        .filter(|target| detected.get(&target.agent).copied().unwrap_or(false))
        .map(|target| target.agent)
        .collect();
    Ok(EnvironmentSnapshot {
        path_exists: records[1] == "1",
        detected_agents,
        skills,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    DefaultActive,
    RequiresPrivateInstall,
    DuplicateCopy,
    PrivateOnly,
    NotInstalled,
}

fn parse_skill_frontmatter(content: &str) -> Option<SkillFrontmatter> {
    let rest = content.strip_prefix("---")?;
    let end = rest.find("---")?;
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(rest[..end].trim()).ok()?;
    (!frontmatter.name.is_empty() && !frontmatter.description.is_empty()).then_some(frontmatter)
}

#[cfg(test)]
mod tests {
    use super::{parse_wsl_inspect_output, WSL_INSPECT_SCRIPT};
    use crate::core::agent_availability::AgentAvailabilityKind;
    use crate::core::agents::AgentType;
    use crate::environment::agent_environment::AgentEnvironmentTarget;

    #[cfg(unix)]
    #[test]
    fn wsl_inspect_script_emits_frontmatter_without_skill_body() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("project");
        let canonical = root.join(".agents/skills/toolkit");
        std::fs::create_dir_all(&canonical).expect("create skill");
        std::fs::write(
            canonical.join("SKILL.md"),
            "---\nname: toolkit\ndescription: Toolkit\n---\nBODY_MUST_NOT_BE_TRANSFERRED\n",
        )
        .expect("write skill");

        let output = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                WSL_INSPECT_SCRIPT,
                "--",
                root.to_string_lossy().as_ref(),
                root.join(".agents/skills").to_string_lossy().as_ref(),
            ])
            .output()
            .expect("run inspect script");

        assert!(output.status.success());
        assert!(!output
            .stdout
            .windows(28)
            .any(|window| { window == b"BODY_MUST_NOT_BE_TRANSFERRED" }));
        let snapshot = parse_wsl_inspect_output(&output.stdout, &[]).expect("parse snapshot");
        assert_eq!(snapshot.skills[0].description, "Toolkit");
    }

    #[test]
    fn parses_versioned_wsl_inspect_records() {
        let snapshot = parse_wsl_inspect_output(
            b"2\x001\0skill\0-\0/home/alice/.agents/skills/toolkit\0---\nname: toolkit\ndescription: Toolkit\n---\n\0skill\0-\0/home/alice/.agents/skills/review\0---\nname: review\ndescription: Review\n---\n\0",
            &[],
        )
        .expect("parse inspect output");

        assert_eq!(snapshot.skills.len(), 2);
        assert!(snapshot.path_exists);
        let review = snapshot
            .skills
            .iter()
            .find(|skill| skill.name == "review")
            .expect("review skill");
        assert_eq!(review.canonical_path, "/home/alice/.agents/skills/review");
    }

    #[test]
    fn rejects_unknown_wsl_inspect_protocol_version() {
        assert!(parse_wsl_inspect_output(b"1\x001\0skill\0-\0/path\0content\0", &[]).is_err());
    }

    #[test]
    fn merges_canonical_and_private_agent_entries_with_presence_summary() {
        let targets = vec![
            AgentEnvironmentTarget {
                agent: AgentType::Codex,
                display_name: "Codex".to_string(),
                shared_path: "/work/app/.agents/skills".to_string(),
                private_path: None,
                availability: AgentAvailabilityKind::SharedOnly,
                default_available: true,
                detection_paths: vec!["/home/alice/.codex".to_string()],
            },
            AgentEnvironmentTarget {
                agent: AgentType::ClaudeCode,
                display_name: "Claude Code".to_string(),
                shared_path: "/work/app/.agents/skills".to_string(),
                private_path: Some("/work/app/.claude/skills".to_string()),
                availability: AgentAvailabilityKind::PrivateRequired,
                default_available: false,
                detection_paths: vec!["/home/alice/.claude".to_string()],
            },
        ];
        let bytes = b"2\x001\0detected\0codex\x001\0detected\0claude-code\x000\0skill\0-\0/work/app/.agents/skills/toolkit\0---\nname: toolkit\ndescription: Shared toolkit\n---\nBody\0skill\0claude-code\0/work/app/.claude/skills/toolkit\0---\nname: toolkit\ndescription: Private toolkit\n---\nBody\0";

        let snapshot = parse_wsl_inspect_output(bytes, &targets).expect("parse inspect output");

        assert!(snapshot.path_exists);
        assert_eq!(snapshot.detected_agents, vec![AgentType::Codex]);
        assert_eq!(snapshot.skills.len(), 1);
        let skill = &snapshot.skills[0];
        assert_eq!(skill.description, "Shared toolkit");
        assert_eq!(skill.canonical_path, "/work/app/.agents/skills/toolkit");
        assert_eq!(skill.agents, vec![AgentType::Codex, AgentType::ClaudeCode]);
        assert_eq!(skill.card_agents, vec![AgentType::Codex]);
        assert_eq!(skill.default_available_agents, vec![AgentType::Codex]);
        assert_eq!(skill.private_adapted_agents, vec![AgentType::ClaudeCode]);
        assert_eq!(skill.private_only_agents, vec![AgentType::ClaudeCode]);
    }

    #[test]
    fn detects_eve_from_project_package_record() {
        let targets = vec![AgentEnvironmentTarget {
            agent: AgentType::Eve,
            display_name: "Eve".to_string(),
            shared_path: "/work/app/.agents/skills".to_string(),
            private_path: Some("/work/app/agent/skills".to_string()),
            availability: AgentAvailabilityKind::PrivateRequired,
            default_available: false,
            detection_paths: vec![
                "/work/app/agent".to_string(),
                "/work/app/package.json".to_string(),
            ],
        }];
        let bytes = b"2\x001\0eve-package\0{\"devDependencies\":{\"eve\":\"^0.11.5\"}}\0";

        let snapshot = parse_wsl_inspect_output(bytes, &targets).expect("parse inspect output");

        assert_eq!(snapshot.detected_agents, vec![AgentType::Eve]);
    }

    #[test]
    fn keeps_eve_root_and_subagent_targets_in_skill_snapshot() {
        let targets = vec![AgentEnvironmentTarget {
            agent: AgentType::Eve,
            display_name: "Eve".to_string(),
            shared_path: "/work/app/.agents/skills".to_string(),
            private_path: Some("/work/app/agent/skills".to_string()),
            availability: AgentAvailabilityKind::PrivateRequired,
            default_available: false,
            detection_paths: vec![
                "/work/app/agent".to_string(),
                "/work/app/package.json".to_string(),
            ],
        }];
        let frontmatter = "---\nname: toolkit\ndescription: Toolkit\n---\n";
        let bytes = format!(
            "2\x001\0eve-package\0{{\"dependencies\":{{\"eve\":\"1\"}}}}\0skill\0eve:root\0/work/app/agent/skills/toolkit\0{frontmatter}\0skill\0eve:research\0/work/app/agent/subagents/research/skills/toolkit\0{frontmatter}\0"
        );

        let snapshot =
            parse_wsl_inspect_output(bytes.as_bytes(), &targets).expect("parse inspect output");
        let skill = &snapshot.skills[0];
        let ids: Vec<_> = skill
            .eve_targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect();

        assert_eq!(ids, vec!["eve:root", "eve:research"]);
    }

    #[test]
    fn eve_subagent_without_root_is_fallback_adapted_not_private_only() {
        let targets = vec![AgentEnvironmentTarget {
            agent: AgentType::Eve,
            display_name: "Eve".to_string(),
            shared_path: "/work/app/.agents/skills".to_string(),
            private_path: Some("/work/app/agent/skills".to_string()),
            availability: AgentAvailabilityKind::PrivateRequired,
            default_available: false,
            detection_paths: Vec::new(),
        }];
        let bytes = b"2\x001\0detected\0eve\x001\0skill\0eve:research\0/work/app/agent/subagents/research/skills/toolkit\0---\nname: toolkit\ndescription: Toolkit\n---\n\0";

        let snapshot = parse_wsl_inspect_output(bytes, &targets).expect("parse inspect output");
        let skill = &snapshot.skills[0];

        assert!(skill.private_only_agents.is_empty());
        assert_eq!(skill.private_adapted_agents, vec![AgentType::Eve]);
        assert_eq!(skill.agents, vec![AgentType::Eve]);
    }
}
