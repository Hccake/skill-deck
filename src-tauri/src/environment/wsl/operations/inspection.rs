use std::future::Future;
use std::time::Duration;

use crate::environment::inspection::{
    FilesystemEntryKind, FilesystemInspector, InspectionFuture, MetadataFuture,
    RawFilesystemSnapshot, RawPathFact, RawSkillMetadata, ReadPlan, ReadRootPurpose,
    SkillMetadataSource,
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
            let environment = FilesystemInspector::environment(self);
            if !same_environment_identity(&plan.context.environment, &environment) {
                return Err(AppError::StorageUnsupported {
                    path: "wslInspector".to_string(),
                });
            }
            let response =
                inspect_plan_with(plan, |request| self.workspace.inspect_filesystem(request))
                    .await?;
            snapshot_from_inspection_response(environment, response, plan.roots.len())
        })
    }
}

impl SkillMetadataSource for WslInspector {
    fn read<'a>(
        &'a self,
        locators: &'a [crate::environment::types::ResourceLocator],
        per_file_limit: u32,
    ) -> MetadataFuture<'a, Result<Vec<RawSkillMetadata>, AppError>> {
        Box::pin(async move {
            let environment = <Self as FilesystemInspector>::environment(self);
            if per_file_limit == 0
                || locators
                    .iter()
                    .any(|locator| !same_environment_identity(&locator.environment, &environment))
            {
                return Err(AppError::StorageUnsupported {
                    path: "wslSkillMetadata".to_string(),
                });
            }
            let facts = self
                .workspace
                .inspect_path_metadata(
                    locators
                        .iter()
                        .map(|locator| {
                            crate::environment::wsl::operations::path_metadata::PathMetadataQuery {
                                path: locator.native_path.clone(),
                                content_limit: Some(per_file_limit),
                            }
                        })
                        .collect(),
                )
                .await?;
            Ok(locators
                .iter()
                .cloned()
                .zip(facts)
                .map(|(locator, fact)| {
                    use crate::environment::wsl::operations::path_metadata::{
                        PathMetadataContent, PathMetadataKind,
                    };
                    let readable_file = matches!(
                        fact.content,
                        PathMetadataContent::Bytes(_) | PathMetadataContent::Empty
                    );
                    let (bytes, error_code) = match fact.content {
                        PathMetadataContent::Bytes(bytes) => (bytes, None),
                        PathMetadataContent::Empty => (Vec::new(), None),
                        PathMetadataContent::Unreadable => {
                            (Vec::new(), Some("readFailed".to_string()))
                        }
                        PathMetadataContent::NotRequested => {
                            (Vec::new(), Some("invalidMetadataResponse".to_string()))
                        }
                    };
                    let error_code = error_code.or_else(|| match fact.kind {
                        PathMetadataKind::Missing => Some("missing".to_string()),
                        PathMetadataKind::Inaccessible => Some("pathUnavailable".to_string()),
                        PathMetadataKind::Directory
                        | PathMetadataKind::SymlinkDirectory
                        | PathMetadataKind::SymlinkOther
                        | PathMetadataKind::BrokenLink => Some("notFile".to_string()),
                        PathMetadataKind::Other if !readable_file => Some("notFile".to_string()),
                        PathMetadataKind::Other => None,
                    });
                    RawSkillMetadata {
                        locator,
                        bytes,
                        truncated: fact.truncated,
                        error_code,
                    }
                })
                .collect())
        })
    }
}

async fn inspect_plan_with<Send, SendFuture>(
    plan: &ReadPlan,
    mut send: Send,
) -> Result<environment_protocol::InspectionResponse, AppError>
where
    Send: FnMut(environment_protocol::InspectionRequest) -> SendFuture,
    SendFuture: Future<Output = Result<environment_protocol::InspectionResponse, AppError>>,
{
    if plan.roots.len() > plan.aggregate_limit as usize {
        return Err(AppError::CapabilityUnavailable {
            capability: "wslInspectionRootCapacity".to_string(),
            path: None,
        });
    }
    let deadline = tokio::time::Instant::now() + Duration::from_millis(INSPECTION_DEADLINE_MILLIS);
    let mut facts = Vec::new();
    let mut remaining_content = plan.aggregate_limit;
    for (batch_index, batch) in plan
        .roots
        .chunks(environment_protocol::MAX_INSPECTION_ROOTS)
        .enumerate()
    {
        let offset = batch_index * environment_protocol::MAX_INSPECTION_ROOTS;
        let remaining_roots = plan.roots.len() - offset;
        let batch_content = ((u64::from(remaining_content) * batch.len() as u64)
            / remaining_roots as u64)
            .max(1) as u32;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let deadline_millis = u64::try_from(remaining.as_millis())
            .unwrap_or(environment_protocol::MAX_REQUEST_DEADLINE_MILLIS)
            .min(environment_protocol::MAX_REQUEST_DEADLINE_MILLIS);
        if deadline_millis == 0 {
            return Err(AppError::WslCommandTimedOut);
        }
        let mut response = tokio::time::timeout_at(
            deadline,
            send(environment_protocol::InspectionRequest {
                read_content: false,
                roots: batch
                    .iter()
                    .map(|root| environment_protocol::InspectionRoot {
                        path: root.locator.native_path.clone(),
                        stat_only: root.purposes.len() == 1
                            && root.purposes.contains(&ReadRootPurpose::Context),
                    })
                    .collect(),
                per_file_limit: plan.per_file_limit.min(batch_content),
                aggregate_limit: batch_content,
                deadline_millis,
            }),
        )
        .await
        .map_err(|_| AppError::WslCommandTimedOut)??;
        if response.total_content_bytes > remaining_content
            || response
                .facts
                .iter()
                .any(|fact| fact.root_index as usize >= batch.len())
        {
            return Err(worker_protocol_error(
                "worker inspection batch exceeded its request contract",
            ));
        }
        remaining_content -= response.total_content_bytes;
        for fact in &mut response.facts {
            fact.root_index += offset as u32;
        }
        facts.append(&mut response.facts);
    }
    Ok(environment_protocol::InspectionResponse {
        total_content_bytes: plan.aggregate_limit - remaining_content,
        facts,
    })
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
                fingerprint: fact
                    .fingerprint
                    .map(crate::environment::runtime::EntryFingerprint),
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
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

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
                fingerprint: Some("entry-v1-demo".to_string()),
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
                fingerprint: None,
                content_bytes: Vec::new(),
                truncated: false,
                error_code: None,
            }],
            total_content_bytes: 0,
        };

        assert!(snapshot_from_inspection_response(EnvironmentRef::Native, response, 1).is_err());
    }

    #[tokio::test]
    async fn inspection_batches_preserve_global_root_indices_and_content_budget() {
        use crate::environment::runtime::ContextSnapshotRevision;
        use crate::environment::types::{ResourceLocator, SkillLocation, SkillLocationRef};

        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let plan = ReadPlan {
            context: SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Global,
            },
            roots: (0..513)
                .map(|index| crate::environment::inspection::ReadRoot {
                    locator: ResourceLocator {
                        environment: environment.clone(),
                        native_path: format!("/tmp/root-{index}"),
                    },
                    purposes: BTreeSet::from([ReadRootPurpose::Private]),
                    consumer_agent_ids: BTreeSet::new(),
                })
                .collect(),
            registry_revision: "registry-v1".to_string(),
            environment_revision: "environment-v1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1").unwrap(),
            per_file_limit: 1024,
            aggregate_limit: 4096,
        };
        let batches = Arc::new(Mutex::new(Vec::new()));
        let observed = batches.clone();
        let response = inspect_plan_with(&plan, move |request| {
            observed
                .lock()
                .unwrap()
                .push((request.roots.len(), request.aggregate_limit));
            async move {
                let total_content_bytes = request.aggregate_limit;
                Ok(environment_protocol::InspectionResponse {
                    facts: request
                        .roots
                        .into_iter()
                        .enumerate()
                        .map(|(root_index, _)| environment_protocol::InspectionFact {
                            root_index: root_index as u32,
                            relative_path: Vec::new(),
                            kind: environment_protocol::InspectionEntryKind::Missing,
                            resolved_target: None,
                            fingerprint: None,
                            content_bytes: Vec::new(),
                            truncated: false,
                            error_code: None,
                        })
                        .collect(),
                    total_content_bytes,
                })
            }
        })
        .await
        .unwrap();

        let batches = batches.lock().unwrap();
        assert_eq!(
            batches.iter().map(|batch| batch.0).collect::<Vec<_>>(),
            vec![256, 256, 1]
        );
        assert!(batches.iter().map(|batch| batch.1).sum::<u32>() <= plan.aggregate_limit);
        assert_eq!(response.facts.len(), plan.roots.len());
        assert_eq!(response.facts[256].root_index, 256);
        assert_eq!(response.facts[512].root_index, 512);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "requires SKILL_DECK_TEST_WSL_DISTRO and a matching real WSL Worker"]
    async fn real_wsl_worker_inspects_more_than_one_root_batch() {
        use crate::environment::runtime::ContextSnapshotRevision;
        use crate::environment::types::{ResourceLocator, SkillLocation, SkillLocationRef};
        use crate::environment::wsl::WslRuntime;

        let distro_name =
            std::env::var("SKILL_DECK_TEST_WSL_DISTRO").expect("set SKILL_DECK_TEST_WSL_DISTRO");
        let runtime = WslRuntime::for_wsl_test();
        runtime.connect(&distro_name).await.expect("connect Worker");
        let environment = EnvironmentRef::Wsl {
            distro_name: distro_name.clone(),
        };
        let plan = ReadPlan {
            context: SkillLocationRef {
                environment: environment.clone(),
                scope: SkillLocation::Global,
            },
            roots: (0..513)
                .map(|index| crate::environment::inspection::ReadRoot {
                    locator: ResourceLocator {
                        environment: environment.clone(),
                        native_path: format!("/tmp/skill-deck-root-batch-{index}"),
                    },
                    purposes: BTreeSet::from([ReadRootPurpose::Private]),
                    consumer_agent_ids: BTreeSet::new(),
                })
                .collect(),
            registry_revision: "registry-v1".to_string(),
            environment_revision: "environment-v1".to_string(),
            context_revision: ContextSnapshotRevision::parse("context-v1").unwrap(),
            per_file_limit: 1024,
            aggregate_limit: 4096,
        };

        let snapshot = WslInspector::new(runtime.workspace(&distro_name).unwrap())
            .inspect(&plan)
            .await
            .expect("inspect all roots");

        assert_eq!(snapshot.facts.len(), plan.roots.len());
        assert_eq!(snapshot.facts[256].root_index, 256);
        assert_eq!(snapshot.facts[512].root_index, 512);
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
