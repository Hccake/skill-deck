use crate::environment::inspection::{
    FilesystemEntryKind, FilesystemInspector, InspectionFuture, RawFilesystemSnapshot, RawPathFact,
    ReadPlan, ReadRootPurpose,
};
use crate::environment::types::{same_environment_identity, EnvironmentRef};
use crate::environment::wsl::operations::scan::{
    self, ScanRequest, ScanResponse, ScannedEntryKind,
};
use crate::environment::wsl::WslSession;
use crate::error::AppError;

pub struct WslInspector {
    session: WslSession,
}

impl WslInspector {
    pub fn new(session: WslSession) -> Self {
        Self { session }
    }
}

impl FilesystemInspector for WslInspector {
    fn environment(&self) -> EnvironmentRef {
        EnvironmentRef::Wsl {
            distro_name: self.session.distro_name.clone(),
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
            let response = scan::scan(
                &self.session,
                ScanRequest {
                    roots: plan
                        .roots
                        .iter()
                        .map(|root| root.locator.native_path.clone())
                        .collect(),
                    stat_only_root_indexes: plan
                        .roots
                        .iter()
                        .enumerate()
                        .filter(|(_, root)| {
                            root.purposes.len() == 1
                                && root.purposes.contains(&ReadRootPurpose::Context)
                        })
                        .map(|(index, _)| index as u32)
                        .collect(),
                    recursive: false,
                    per_file_limit: plan.per_file_limit,
                    aggregate_limit: plan.aggregate_limit,
                },
                None,
            )
            .await?;
            snapshot_from_scan_response(environment, response)
        })
    }
}

pub fn snapshot_from_scan_response(
    environment: EnvironmentRef,
    response: ScanResponse,
) -> Result<RawFilesystemSnapshot, AppError> {
    let facts = response
        .entries
        .into_iter()
        .map(|entry| RawPathFact {
            root_index: entry.root_index,
            relative_path: entry.relative_path,
            kind: match entry.kind {
                ScannedEntryKind::Missing => FilesystemEntryKind::Missing,
                ScannedEntryKind::File => FilesystemEntryKind::File,
                ScannedEntryKind::Directory => FilesystemEntryKind::Directory,
                ScannedEntryKind::Symlink => FilesystemEntryKind::Symlink,
                ScannedEntryKind::Other | ScannedEntryKind::Error => FilesystemEntryKind::Other,
            },
            resolved_target: entry.resolved_target,
            frontmatter_bytes: entry.content_bytes,
            truncated: entry.truncated,
            error_code: entry.error_code,
        })
        .collect();
    Ok(RawFilesystemSnapshot {
        environment,
        facts,
        total_content_bytes: response.total_content_bytes,
    })
}

#[cfg(all(test, target_os = "linux"))]
#[allow(
    clippy::disallowed_methods,
    reason = "inspection 协议测试需要直接执行被验证的 shell fixture"
)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::process::Command;

    use tempfile::tempdir;

    use super::*;
    use crate::core::agent_definition::AgentId;
    use crate::environment::inspection::{FilesystemInspector, ReadPlanBuilder, ReadRootPurpose};
    use crate::environment::native::inspection::NativeInspector;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};
    use crate::environment::wsl::operations::scan::{
        parse_scan_response, ScanRequest, SCAN_SCRIPT,
    };

    #[tokio::test]
    async fn native_and_posix_protocol_project_the_same_filesystem_facts() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("skills");
        fs::create_dir_all(root.join("demo")).unwrap();
        fs::write(root.join("demo/SKILL.md"), b"---\nname: demo\n---\nbody").unwrap();
        let missing = temp.path().join("missing");
        let context = ContextRef {
            environment: EnvironmentRef::Host,
            scope: ContextScope::Global,
        };
        let mut builder = ReadPlanBuilder::new(
            context,
            "registry-1",
            "environment-1",
            ContextSnapshotRevision::parse("context-v1-parity").unwrap(),
        );
        for (index, path) in [&missing, &root].into_iter().enumerate() {
            builder
                .add_root(
                    ResourceLocator {
                        environment: EnvironmentRef::Host,
                        native_path: path.to_string_lossy().into_owned(),
                    },
                    ReadRootPurpose::Detection,
                    Some(AgentId::parse(format!("agent-{index}")).unwrap()),
                )
                .unwrap();
        }
        let plan = builder.build().unwrap();
        let native = NativeInspector::new(EnvironmentRef::Host)
            .inspect(&plan)
            .await
            .expect("native inspect");

        let request = ScanRequest {
            roots: plan
                .roots
                .iter()
                .map(|root| root.locator.native_path.clone())
                .collect(),
            stat_only_root_indexes: BTreeSet::new(),
            recursive: false,
            per_file_limit: plan.per_file_limit,
            aggregate_limit: plan.aggregate_limit,
        };
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(SCAN_SCRIPT)
            .arg("--")
            .arg("scan")
            .arg(request.per_file_limit.to_string())
            .arg(request.aggregate_limit.to_string())
            .arg("")
            .arg("0")
            .args(&request.roots)
            .output()
            .unwrap();
        assert!(output.status.success());
        let posix = snapshot_from_scan_response(
            EnvironmentRef::Host,
            parse_scan_response(&output.stdout, &request).unwrap(),
        )
        .expect("project response");

        assert_eq!(native, posix);
    }
}
