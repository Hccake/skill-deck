use crate::environment::inspection::{
    FilesystemEntryKind, FilesystemInspector, InspectionFuture, RawFilesystemSnapshot, RawPathFact,
    ReadPlan, ReadRootPurpose,
};
use crate::environment::types::{same_environment_identity, EnvironmentRef};
use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const INSPECTION_DEADLINE_MILLIS: u64 = 30_000;

pub struct WslInspector {
    workspace: WslWorkspace,
}

impl WslInspector {
    pub fn new(workspace: WslWorkspace) -> Self {
        Self { workspace }
    }
}

impl FilesystemInspector for WslInspector {
    fn environment(&self) -> EnvironmentRef {
        EnvironmentRef::Wsl {
            distro_name: self.workspace.distro_name().to_string(),
        }
    }

    fn inspect<'a>(
        &'a self,
        plan: &'a ReadPlan,
    ) -> InspectionFuture<'a, Result<RawFilesystemSnapshot, AppError>> {
        Box::pin(async move {
            let environment = self.environment();
            if !same_environment_identity(&plan.context.environment, &environment) {
                return Err(AppError::StorageUnsupported {
                    path: "wslInspector".to_string(),
                });
            }
            let response = self
                .workspace
                .inspect_filesystem(environment_protocol::InspectionRequest {
                    roots: plan
                        .roots
                        .iter()
                        .map(|root| environment_protocol::InspectionRoot {
                            path: root.locator.native_path.clone(),
                            stat_only: root.purposes.len() == 1
                                && root.purposes.contains(&ReadRootPurpose::Context),
                        })
                        .collect(),
                    per_file_limit: plan.per_file_limit,
                    aggregate_limit: plan.aggregate_limit,
                    deadline_millis: INSPECTION_DEADLINE_MILLIS,
                })
                .await?;
            snapshot_from_inspection_response(environment, response, plan.roots.len())
        })
    }
}

pub fn snapshot_from_inspection_response(
    environment: EnvironmentRef,
    response: environment_protocol::InspectionResponse,
    root_count: usize,
) -> Result<RawFilesystemSnapshot, AppError> {
    let total_content_bytes = response
        .facts
        .iter()
        .map(|fact| fact.content_bytes.len())
        .sum::<usize>();
    if total_content_bytes != response.total_content_bytes as usize {
        return Err(worker_protocol_error(
            "worker inspection content total does not match its facts",
        ));
    }
    let facts = response
        .facts
        .into_iter()
        .map(|fact| {
            if fact.root_index as usize >= root_count {
                return Err(worker_protocol_error(
                    "worker inspection root index is out of range",
                ));
            }
            let relative_path = String::from_utf8(fact.relative_path)
                .map_err(|_| worker_protocol_error("worker inspection path is not UTF-8"))?;
            if relative_path.starts_with('/')
                || relative_path.contains('\\')
                || relative_path.split('/').any(|component| component == "..")
            {
                return Err(worker_protocol_error(
                    "worker inspection returned an unsafe relative path",
                ));
            }
            let resolved_target = fact
                .resolved_target
                .map(String::from_utf8)
                .transpose()
                .map_err(|_| worker_protocol_error("worker link target is not UTF-8"))?;
            Ok(RawPathFact {
                root_index: fact.root_index,
                relative_path,
                kind: match fact.kind {
                    environment_protocol::InspectionEntryKind::Missing => {
                        FilesystemEntryKind::Missing
                    }
                    environment_protocol::InspectionEntryKind::File => FilesystemEntryKind::File,
                    environment_protocol::InspectionEntryKind::Directory => {
                        FilesystemEntryKind::Directory
                    }
                    environment_protocol::InspectionEntryKind::Symlink => {
                        FilesystemEntryKind::Symlink
                    }
                    environment_protocol::InspectionEntryKind::Other => FilesystemEntryKind::Other,
                },
                resolved_target,
                frontmatter_bytes: fact.content_bytes,
                truncated: fact.truncated,
                error_code: fact.error_code.map(|code| match code {
                    environment_protocol::InspectionErrorCode::PathUnavailable => {
                        "pathUnavailable".to_string()
                    }
                    environment_protocol::InspectionErrorCode::ReadFailed => {
                        "readFailed".to_string()
                    }
                    environment_protocol::InspectionErrorCode::ReadLinkFailed => {
                        "readLinkFailed".to_string()
                    }
                }),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(RawFilesystemSnapshot {
        environment,
        facts,
        total_content_bytes: response.total_content_bytes,
    })
}

fn worker_protocol_error(message: &str) -> AppError {
    AppError::ConfigurationCorrupted {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_snapshot_projects_bounded_filesystem_facts() {
        let content = b"---\nname: demo\n---\n".to_vec();
        let response = environment_protocol::InspectionResponse {
            facts: vec![environment_protocol::InspectionFact {
                root_index: 0,
                relative_path: b"demo/SKILL.md".to_vec(),
                kind: environment_protocol::InspectionEntryKind::File,
                resolved_target: None,
                content_bytes: content.clone(),
                truncated: false,
                error_code: None,
            }],
            total_content_bytes: content.len() as u32,
        };

        let snapshot = snapshot_from_inspection_response(EnvironmentRef::Native, response, 1)
            .expect("snapshot");

        assert_eq!(snapshot.facts.len(), 1);
        assert_eq!(snapshot.facts[0].relative_path, "demo/SKILL.md");
        assert_eq!(snapshot.facts[0].kind, FilesystemEntryKind::File);
        assert_eq!(snapshot.total_content_bytes, content.len() as u32);
    }

    #[test]
    fn worker_snapshot_rejects_unsafe_relative_paths() {
        let response = environment_protocol::InspectionResponse {
            facts: vec![environment_protocol::InspectionFact {
                root_index: 0,
                relative_path: b"../escape".to_vec(),
                kind: environment_protocol::InspectionEntryKind::File,
                resolved_target: None,
                content_bytes: Vec::new(),
                truncated: false,
                error_code: None,
            }],
            total_content_bytes: 0,
        };

        assert!(snapshot_from_inspection_response(EnvironmentRef::Native, response, 1).is_err());
    }

    #[cfg(target_os = "windows")]
    // Run from the repository root on Windows after preparing the exact Worker build:
    // `$env:SKILL_DECK_TEST_WSL_DISTRO='Ubuntu'; cargo test --manifest-path src-tauri/Cargo.toml environment::wsl::operations::inspection::tests::real_wsl_worker_executes_a_skill_read_plan -- --ignored --exact`
    #[tokio::test]
    #[ignore = "requires SKILL_DECK_TEST_WSL_DISTRO and a real WSL 2 distribution"]
    async fn real_wsl_worker_executes_a_skill_read_plan() {
        use std::time::Duration;

        use crate::environment::inspection::{
            FilesystemInspector, ReadPlanBuilder, ReadRootPurpose,
        };
        use crate::environment::runtime::ContextSnapshotRevision;
        use crate::environment::types::{ResourceLocator, SkillLocation, SkillLocationRef};
        use crate::environment::wsl::protocol::{
            WslCommandRequest, WslCommandRunner, DEFAULT_WSL_STDERR_LIMIT, DEFAULT_WSL_STDOUT_LIMIT,
        };
        use crate::environment::wsl::WslRuntime;

        let distro_name = std::env::var("SKILL_DECK_TEST_WSL_DISTRO")
            .expect("set SKILL_DECK_TEST_WSL_DISTRO to an installed WSL 2 distribution");
        let root = format!("/tmp/skill-deck-worker-inspection-{}", uuid::Uuid::new_v4());
        let runtime = WslRuntime::for_wsl_test();
        let session = runtime
            .connect(&distro_name)
            .await
            .expect("connect WSL Worker");
        let setup = WslCommandRunner::run(WslCommandRequest {
            session: session.clone(),
            script: "set -eu\nroot=$1\nmkdir -p \"$root/demo\"\ncat > \"$root/demo/SKILL.md\"\n",
            args: vec![root.clone()],
            stdin: b"---\nname: demo\ndescription: Worker fixture\n---\n".to_vec(),
            timeout: Duration::from_secs(10),
            stdout_limit: DEFAULT_WSL_STDOUT_LIMIT,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        })
        .await
        .expect("create fixture");
        assert_eq!(setup.exit_code, Some(0));

        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.clone(),
        };
        let mut builder = ReadPlanBuilder::new(
            SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Global,
            },
            "registry-worker-test",
            "environment-worker-test",
            ContextSnapshotRevision::parse("context-v1-worker-test").unwrap(),
        );
        builder
            .add_root(
                ResourceLocator {
                    environment,
                    native_path: root.clone(),
                },
                ReadRootPurpose::Private,
                None,
            )
            .unwrap();
        let snapshot = WslInspector::new(runtime.workspace(&distro_name).unwrap())
            .inspect(&builder.build().unwrap())
            .await;

        let cleanup = WslCommandRunner::run(WslCommandRequest {
            session,
            script: "set -eu\ncase $1 in /tmp/skill-deck-worker-inspection-*) rm -rf -- \"$1\" ;; *) exit 64 ;; esac\n",
            args: vec![root],
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: DEFAULT_WSL_STDOUT_LIMIT,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        })
        .await;

        let snapshot = snapshot.expect("read fixture through WSL Worker");
        cleanup.expect("clean fixture");
        let document = snapshot
            .facts
            .iter()
            .find(|fact| fact.relative_path == "demo/SKILL.md")
            .expect("Skill document fact");
        assert_eq!(document.kind, FilesystemEntryKind::File);
        assert!(document.frontmatter_bytes.starts_with(b"---\nname: demo\n"));
    }
}
