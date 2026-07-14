use tokio::time::Duration;

use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::run_wsl_script;
use crate::error::AppError;
use crate::models::InstallMode;

const WSL_MATERIALIZE_SCRIPT: &str = r#"
source_path=$1
canonical_root=$2
skill_name=$3
context_root=$4
shift 4

[ -d "$context_root" ] || { printf 'Context directory not found: %s\n' "$context_root" >&2; exit 9; }
mkdir -p -- "$canonical_root"
canonical_path=$canonical_root/$skill_name
canonical_tmp=$(mktemp -d "$canonical_root/.skill-deck.XXXXXX") || exit 10
trap 'rm -rf -- "$canonical_tmp"' EXIT HUP INT TERM
cp -a -- "$source_path"/. "$canonical_tmp"/ || exit 11
rm -rf -- "$canonical_path" || exit 12
mv -- "$canonical_tmp" "$canonical_path" || exit 13
trap - EXIT HUP INT TERM

printf '1\0canonical\0%s\0' "$canonical_path"

while [ "$#" -ge 6 ]; do
  target_id=$1
  agent=$2
  skills_root=$3
  mode=$4
  required_root=$5
  preserve_existing_mode=$6
  shift 6
  target_path=$skills_root/$skill_name
  status=success
  symlink_failed=0
  error=

  if [ "$preserve_existing_mode" = 1 ]; then
    if [ -L "$target_path" ]; then
      mode=symlink
    else
      mode=copy
    fi
  fi

  if [ -n "$required_root" ] && [ ! -d "$required_root" ]; then
    status=skipped
    error='agent root is unavailable'
  else
    mkdir -p -- "$skills_root" 2>/dev/null || {
      status=failed
      error='failed to create agent skills directory'
    }
    if [ "$status" = success ]; then
      rm -rf -- "$target_path" 2>/dev/null || {
        status=failed
        error='failed to remove existing agent target'
      }
    fi
    if [ "$status" = success ] && [ "$mode" = symlink ]; then
      if ! ln -s -- "$canonical_path" "$target_path" 2>/dev/null; then
        symlink_failed=1
        rm -rf -- "$target_path" 2>/dev/null || true
        if ! cp -a -- "$canonical_path" "$target_path" 2>/dev/null; then
          status=failed
          error='failed to create symlink or fallback copy'
        fi
      fi
    elif [ "$status" = success ]; then
      if ! cp -a -- "$canonical_path" "$target_path" 2>/dev/null; then
        status=failed
        error='failed to copy skill to agent target'
      fi
    fi
  fi

  printf 'target\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0' \
    "$target_id" "$agent" "$status" "$target_path" "$mode" "$symlink_failed" "$error"
done
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslMaterializeTarget {
    pub target_id: String,
    pub agent: String,
    pub skills_root: String,
    pub mode: InstallMode,
    pub required_root: Option<String>,
    pub preserve_existing_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslMaterializeRequest {
    pub source_skill_path: String,
    pub canonical_root: String,
    pub install_dir_name: String,
    pub context_root: String,
    pub canonical_mode: InstallMode,
    pub targets: Vec<WslMaterializeTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslMaterializePlan {
    pub script: &'static str,
    pub positional_args: Vec<String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslMaterializeTargetResult {
    pub target_id: String,
    pub agent: String,
    pub success: bool,
    pub skipped: bool,
    pub path: String,
    pub mode: InstallMode,
    pub symlink_failed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslMaterializeResult {
    pub canonical_path: String,
    pub canonical_mode: InstallMode,
    pub targets: Vec<WslMaterializeTargetResult>,
}

pub fn build_wsl_materialize_plan(request: &WslMaterializeRequest) -> WslMaterializePlan {
    let mut positional_args = vec![
        request.source_skill_path.clone(),
        request.canonical_root.clone(),
        request.install_dir_name.clone(),
        request.context_root.clone(),
    ];
    for target in &request.targets {
        positional_args.push(target.target_id.clone());
        positional_args.push(target.agent.clone());
        positional_args.push(target.skills_root.clone());
        positional_args.push(match target.mode {
            InstallMode::Symlink => "symlink".to_string(),
            InstallMode::Copy => "copy".to_string(),
        });
        positional_args.push(target.required_root.clone().unwrap_or_default());
        positional_args.push(if target.preserve_existing_mode {
            "1".to_string()
        } else {
            "0".to_string()
        });
    }
    WslMaterializePlan {
        script: WSL_MATERIALIZE_SCRIPT,
        positional_args,
        timeout: Duration::from_secs(120),
    }
}

pub fn parse_wsl_materialize_output(bytes: &[u8]) -> Result<WslMaterializeResult, AppError> {
    let mut records = bytes
        .split(|byte| *byte == 0)
        .map(|record| String::from_utf8_lossy(record).into_owned())
        .collect::<Vec<_>>();
    if records.last().is_some_and(String::is_empty) {
        records.pop();
    }
    if records.first().map(String::as_str) != Some("1") {
        return Err(AppError::Custom {
            message: "invalid WSL materialize response version".to_string(),
        });
    }
    let mut canonical_path = None;
    let mut targets = Vec::new();
    let mut index = 1;
    while index < records.len() {
        match records[index].as_str() {
            "canonical" if index + 1 < records.len() => {
                canonical_path = Some(records[index + 1].clone());
                index += 2;
            }
            "target" if index + 7 < records.len() => {
                let status = records[index + 3].as_str();
                let mode = match records[index + 5].as_str() {
                    "symlink" => InstallMode::Symlink,
                    "copy" => InstallMode::Copy,
                    _ => {
                        return Err(AppError::Custom {
                            message: "invalid WSL materialize mode".to_string(),
                        })
                    }
                };
                let symlink_failed = match records[index + 6].as_str() {
                    "0" => false,
                    "1" => true,
                    _ => {
                        return Err(AppError::Custom {
                            message: "invalid WSL materialize symlink flag".to_string(),
                        })
                    }
                };
                targets.push(WslMaterializeTargetResult {
                    target_id: records[index + 1].clone(),
                    agent: records[index + 2].clone(),
                    success: status != "failed",
                    skipped: status == "skipped",
                    path: records[index + 4].clone(),
                    mode,
                    symlink_failed,
                    error: (!records[index + 7].is_empty()).then(|| records[index + 7].clone()),
                });
                if !matches!(status, "success" | "skipped" | "failed") {
                    return Err(AppError::Custom {
                        message: "invalid WSL materialize target status".to_string(),
                    });
                }
                index += 8;
            }
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL materialize response".to_string(),
                })
            }
        }
    }
    Ok(WslMaterializeResult {
        canonical_path: canonical_path.ok_or_else(|| AppError::Custom {
            message: "WSL materialize response is missing canonical path".to_string(),
        })?,
        canonical_mode: InstallMode::Copy,
        targets,
    })
}

async fn materialize_wsl_skill_with<F, Fut>(
    session: WslSession,
    request: WslMaterializeRequest,
    runner: F,
) -> Result<WslMaterializeResult, AppError>
where
    F: FnOnce(WslSession, &'static str, Vec<String>, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, AppError>>,
{
    let canonical_mode = request.canonical_mode.clone();
    let plan = build_wsl_materialize_plan(&request);
    let output = runner(session, plan.script, plan.positional_args, plan.timeout).await?;
    let mut result = parse_wsl_materialize_output(&output)?;
    result.canonical_mode = canonical_mode;
    Ok(result)
}

pub async fn materialize_wsl_skill(
    session: &WslSession,
    request: WslMaterializeRequest,
) -> Result<WslMaterializeResult, AppError> {
    materialize_wsl_skill_with(
        session.clone(),
        request,
        |session, script, positional_args, timeout| async move {
            run_wsl_script(&session, script, &positional_args, Vec::new(), timeout).await
        },
    )
    .await
}

const WSL_AGENT_MATERIALIZE_SCRIPT: &str = r#"
canonical=$1
shift
[ -d "$canonical" ] || { printf 'Canonical skill not found: %s\n' "$canonical" >&2; exit 9; }
printf '1\0'

while [ "$#" -ge 5 ]; do
  target_id=$1
  agent=$2
  target_path=$3
  mode=$4
  protect_existing_copy=$5
  shift 5
  status=success
  error=

  if [ "$protect_existing_copy" = 1 ] && { [ -e "$target_path" ] || [ -L "$target_path" ]; } && [ ! -L "$target_path" ]; then
    status=failed
    error='private copy already exists'
  else
    parent=${target_path%/*}
    mkdir -p -- "$parent" 2>/dev/null || {
      status=failed
      error='failed to create agent skills directory'
    }
    if [ "$status" = success ]; then
      rm -rf -- "$target_path" 2>/dev/null || {
        status=failed
        error='failed to remove existing agent target'
      }
    fi
    if [ "$status" = success ] && [ "$mode" = symlink ]; then
      if ! ln -s -- "$canonical" "$target_path" 2>/dev/null; then
        status=failed
        error='failed to create symlink'
      fi
    elif [ "$status" = success ]; then
      if ! cp -a -- "$canonical" "$target_path" 2>/dev/null; then
        status=failed
        error='failed to copy skill'
      fi
    fi
  fi

  printf 'target\0%s\0%s\0%s\0%s\0%s\0%s\0' \
    "$target_id" "$agent" "$status" "$target_path" "$mode" "$error"
done
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslAgentMaterializeTarget {
    pub target_id: String,
    pub agent: String,
    pub target_path: String,
    pub mode: InstallMode,
    pub protect_existing_copy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslAgentMaterializeRequest {
    pub canonical_path: String,
    pub targets: Vec<WslAgentMaterializeTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslAgentMaterializePlan {
    pub script: &'static str,
    pub positional_args: Vec<String>,
    pub timeout: Duration,
}

pub fn build_wsl_agent_materialize_plan(
    request: &WslAgentMaterializeRequest,
) -> WslAgentMaterializePlan {
    let mut positional_args = vec![request.canonical_path.clone()];
    for target in &request.targets {
        positional_args.push(target.target_id.clone());
        positional_args.push(target.agent.clone());
        positional_args.push(target.target_path.clone());
        positional_args.push(match target.mode {
            InstallMode::Symlink => "symlink".to_string(),
            InstallMode::Copy => "copy".to_string(),
        });
        positional_args.push(if target.protect_existing_copy {
            "1".to_string()
        } else {
            "0".to_string()
        });
    }
    WslAgentMaterializePlan {
        script: WSL_AGENT_MATERIALIZE_SCRIPT,
        positional_args,
        timeout: Duration::from_secs(60),
    }
}

pub fn parse_wsl_agent_materialize_output(
    bytes: &[u8],
) -> Result<Vec<WslMaterializeTargetResult>, AppError> {
    let mut records = bytes
        .split(|byte| *byte == 0)
        .map(|record| String::from_utf8_lossy(record).into_owned())
        .collect::<Vec<_>>();
    if records.last().is_some_and(String::is_empty) {
        records.pop();
    }
    if records.first().map(String::as_str) != Some("1") {
        return Err(AppError::Custom {
            message: "invalid WSL agent materialize response version".to_string(),
        });
    }
    let mut results = Vec::new();
    let mut index = 1;
    while index < records.len() {
        if records.get(index).map(String::as_str) != Some("target") || index + 6 >= records.len() {
            return Err(AppError::Custom {
                message: "invalid WSL agent materialize response".to_string(),
            });
        }
        let success = match records[index + 3].as_str() {
            "success" => true,
            "failed" => false,
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL agent materialize status".to_string(),
                })
            }
        };
        let mode = match records[index + 5].as_str() {
            "symlink" => InstallMode::Symlink,
            "copy" => InstallMode::Copy,
            _ => {
                return Err(AppError::Custom {
                    message: "invalid WSL agent materialize mode".to_string(),
                })
            }
        };
        results.push(WslMaterializeTargetResult {
            target_id: records[index + 1].clone(),
            agent: records[index + 2].clone(),
            success,
            skipped: false,
            path: records[index + 4].clone(),
            mode,
            symlink_failed: false,
            error: (!records[index + 6].is_empty()).then(|| records[index + 6].clone()),
        });
        index += 7;
    }
    Ok(results)
}

pub async fn materialize_wsl_agent_targets(
    session: &WslSession,
    request: WslAgentMaterializeRequest,
) -> Result<Vec<WslMaterializeTargetResult>, AppError> {
    if request.targets.is_empty() {
        return Ok(Vec::new());
    }
    let plan = build_wsl_agent_materialize_plan(&request);
    let output = run_wsl_script(
        session,
        plan.script,
        &plan.positional_args,
        Vec::new(),
        plan.timeout,
    )
    .await?;
    parse_wsl_agent_materialize_output(&output)
}

#[cfg(test)]
mod tests {
    use super::{
        build_wsl_agent_materialize_plan, build_wsl_materialize_plan, materialize_wsl_skill_with,
        parse_wsl_agent_materialize_output, parse_wsl_materialize_output,
        WslAgentMaterializeRequest, WslAgentMaterializeTarget, WslMaterializeRequest,
        WslMaterializeTarget,
    };
    use crate::environment::wsl::WslSession;
    use crate::models::InstallMode;

    fn session() -> WslSession {
        WslSession {
            distro_name: "Ubuntu-24.04".to_string(),
            user: "alice".to_string(),
            uid: 1000,
            home: "/home/alice".to_string(),
            xdg_state_home: None,
            config_home: "/home/alice/.config".to_string(),
            environment: Default::default(),
            git_available: true,
        }
    }

    fn request() -> WslMaterializeRequest {
        WslMaterializeRequest {
            source_skill_path: "/mnt/c/Temp/repo/skills/toolkit".to_string(),
            canonical_root: "/home/alice/.agents/skills".to_string(),
            context_root: "/home/alice".to_string(),
            install_dir_name: "toolkit".to_string(),
            canonical_mode: InstallMode::Copy,
            targets: vec![WslMaterializeTarget {
                target_id: "claude-code".to_string(),
                agent: "claude-code".to_string(),
                skills_root: "/home/alice/.claude/skills".to_string(),
                mode: InstallMode::Symlink,
                required_root: Some("/home/alice/.claude".to_string()),
                preserve_existing_mode: true,
            }],
        }
    }

    #[test]
    fn materialize_plan_keeps_paths_and_targets_positional() {
        let request = request();

        let plan = build_wsl_materialize_plan(&request);

        assert_eq!(plan.positional_args[0], request.source_skill_path);
        assert_eq!(plan.positional_args[1], request.canonical_root);
        assert_eq!(plan.positional_args[2], request.install_dir_name);
        assert_eq!(plan.positional_args[3], "/home/alice");
        assert_eq!(plan.positional_args[4], "claude-code");
        assert_eq!(plan.positional_args[5], "claude-code");
        assert_eq!(plan.positional_args[6], "/home/alice/.claude/skills");
        assert_eq!(plan.positional_args[7], "symlink");
        assert_eq!(plan.positional_args[8], "/home/alice/.claude");
        assert_eq!(plan.positional_args[9], "1");
        assert!(plan.script.contains("[ -d \"$context_root\" ]"));
        assert!(!plan.script.contains("/home/alice/.claude/skills"));
    }

    #[test]
    fn materialize_output_preserves_agent_partial_results() {
        let output = b"1\0canonical\0/home/alice/.agents/skills/toolkit\0target\0claude-code\0claude-code\0success\0/home/alice/.claude/skills/toolkit\0symlink\x001\0\0target\0eve:research\0eve\0failed\0/home/alice/app/agent/subagents/research/skills/toolkit\0copy\x000\0permission denied\0target\0amp\0amp\0skipped\0/home/alice/.config/agents/skills/toolkit\0copy\x000\0agent root is unavailable\0";

        let result = parse_wsl_materialize_output(output).expect("parse result");

        assert_eq!(result.canonical_path, "/home/alice/.agents/skills/toolkit");
        assert_eq!(result.targets.len(), 3);
        assert!(result.targets[0].success);
        assert!(result.targets[0].symlink_failed);
        assert!(!result.targets[1].success);
        assert!(!result.targets[1].skipped);
        assert_eq!(
            result.targets[1].error.as_deref(),
            Some("permission denied")
        );
        assert!(result.targets[2].skipped);
        assert!(result.targets[2].success);
    }

    #[test]
    fn materialize_output_rejects_unknown_protocol_or_truncated_target() {
        assert!(parse_wsl_materialize_output(b"2\0canonical\0/x\0").is_err());
        assert!(parse_wsl_materialize_output(b"1\0target\0only-id\0").is_err());
    }

    #[tokio::test]
    async fn materialize_runner_parses_real_partial_protocol() {
        let result = materialize_wsl_skill_with(session(), request(), |_, script, args, _| async move {
            assert!(!script.contains("/home/alice/.claude/skills"));
            assert_eq!(args[6], "/home/alice/.claude/skills");
            Ok(b"1\0canonical\0/home/alice/.agents/skills/toolkit\0target\0claude-code\0claude-code\0failed\0/home/alice/.claude/skills/toolkit\0symlink\x000\0permission denied\0".to_vec())
        })
        .await
        .expect("materialize result");

        assert_eq!(result.targets.len(), 1);
        assert!(!result.targets[0].success);
        assert_eq!(
            result.targets[0].error.as_deref(),
            Some("permission denied")
        );
    }

    #[test]
    fn agent_materialize_plan_preserves_mode_and_private_copy_protection() {
        let request = WslAgentMaterializeRequest {
            canonical_path: "/work/.agents/skills/toolkit".to_string(),
            targets: vec![WslAgentMaterializeTarget {
                target_id: "claude-code".to_string(),
                agent: "claude-code".to_string(),
                target_path: "/work/.claude/skills/toolkit".to_string(),
                mode: InstallMode::Copy,
                protect_existing_copy: true,
            }],
        };

        let plan = build_wsl_agent_materialize_plan(&request);

        assert_eq!(plan.positional_args[0], request.canonical_path);
        assert_eq!(plan.positional_args[1], "claude-code");
        assert_eq!(plan.positional_args[3], "/work/.claude/skills/toolkit");
        assert_eq!(plan.positional_args[4], "copy");
        assert_eq!(plan.positional_args[5], "1");
        assert!(!plan.script.contains("/work/.claude/skills/toolkit"));
    }

    #[test]
    fn agent_materialize_output_preserves_partial_results_without_fallback() {
        let output = b"1\0target\0claude-code\0claude-code\0success\0/work/.claude/skills/toolkit\0symlink\0\0target\0amp\0amp\0failed\0/work/.config/agents/skills/toolkit\0copy\0private copy already exists\0";

        let results = parse_wsl_agent_materialize_output(output).expect("parse agent results");

        assert_eq!(results.len(), 2);
        assert!(results[0].success);
        assert!(!results[0].symlink_failed);
        assert!(!results[1].success);
        assert_eq!(
            results[1].error.as_deref(),
            Some("private copy already exists")
        );
    }
}
