use std::sync::Arc;

use crate::environment::inspection::{FilesystemInspector, RawFilesystemSnapshot, ReadPlan};
use crate::environment::types::same_environment_identity;
use crate::error::AppError;

pub struct ReadService {
    inspectors: Vec<Arc<dyn FilesystemInspector>>,
}

impl ReadService {
    pub fn new(inspectors: Vec<Arc<dyn FilesystemInspector>>) -> Self {
        Self { inspectors }
    }

    pub async fn execute(&self, plan: &ReadPlan) -> Result<RawFilesystemSnapshot, AppError> {
        let inspector = self
            .inspectors
            .iter()
            .find(|inspector| {
                same_environment_identity(&inspector.environment(), &plan.context.environment)
            })
            .ok_or_else(|| AppError::EnvironmentUnavailable {
                environment: plan.context.environment.clone(),
                message: "filesystem inspector is unavailable".to_string(),
            })?;
        let snapshot = inspector.inspect(plan).await?;
        validate_snapshot(plan, &snapshot)?;
        Ok(snapshot)
    }
}

fn validate_snapshot(plan: &ReadPlan, snapshot: &RawFilesystemSnapshot) -> Result<(), AppError> {
    if !same_environment_identity(&snapshot.environment, &plan.context.environment)
        || snapshot.total_content_bytes > plan.aggregate_limit
        || snapshot.facts.iter().any(|fact| {
            fact.root_index as usize >= plan.roots.len()
                || fact.frontmatter_bytes.len() > plan.per_file_limit as usize
        })
    {
        return Err(AppError::ConfigurationCorrupted {
            message: "filesystem inspector violated the ReadPlan boundary".to_string(),
        });
    }
    let actual_total = snapshot
        .facts
        .iter()
        .map(|fact| fact.frontmatter_bytes.len() as u32)
        .sum::<u32>();
    if actual_total != snapshot.total_content_bytes {
        return Err(AppError::ConfigurationCorrupted {
            message: "filesystem inspector content total does not match".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::core::agent_definition::AgentId;
    use crate::environment::inspection::*;
    use crate::environment::runtime::ContextSnapshotRevision;
    use crate::environment::types::{ContextRef, ContextScope, EnvironmentRef, ResourceLocator};

    struct FakeInspector {
        environment: EnvironmentRef,
        calls: AtomicUsize,
        oversized: bool,
    }

    impl FilesystemInspector for FakeInspector {
        fn environment(&self) -> EnvironmentRef {
            self.environment.clone()
        }

        fn inspect<'a>(
            &'a self,
            plan: &'a ReadPlan,
        ) -> InspectionFuture<'a, Result<RawFilesystemSnapshot, AppError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let content = if self.oversized {
                    vec![0; plan.per_file_limit as usize + 1]
                } else {
                    Vec::new()
                };
                Ok(RawFilesystemSnapshot {
                    environment: self.environment.clone(),
                    facts: plan
                        .roots
                        .iter()
                        .enumerate()
                        .map(|(index, _)| RawPathFact {
                            root_index: index as u32,
                            relative_path: String::new(),
                            kind: if index == 1 {
                                FilesystemEntryKind::Missing
                            } else {
                                FilesystemEntryKind::Directory
                            },
                            resolved_target: None,
                            frontmatter_bytes: content.clone(),
                            truncated: self.oversized,
                            error_code: (index == 1).then(|| "pathUnavailable".to_string()),
                        })
                        .collect(),
                    total_content_bytes: content.len() as u32 * plan.roots.len() as u32,
                })
            })
        }
    }

    fn context(environment: EnvironmentRef) -> ContextRef {
        ContextRef {
            environment,
            scope: ContextScope::Global,
        }
    }

    fn locator(environment: &EnvironmentRef, path: &str) -> ResourceLocator {
        ResourceLocator {
            environment: environment.clone(),
            native_path: path.to_string(),
        }
    }

    fn builder(environment: EnvironmentRef) -> ReadPlanBuilder {
        ReadPlanBuilder::new(
            context(environment),
            "registry-1",
            "environment-1",
            ContextSnapshotRevision::parse("context-v1-read").unwrap(),
        )
    }

    #[tokio::test]
    async fn one_hundred_agents_sharing_one_root_produce_one_root_and_one_scan() {
        let environment = EnvironmentRef::Host;
        let mut builder = builder(environment.clone());
        for index in 0..100 {
            builder
                .add_root(
                    locator(&environment, "/shared/.agents/skills"),
                    ReadRootPurpose::Private,
                    Some(AgentId::parse(format!("agent-{index}")).unwrap()),
                )
                .unwrap();
        }
        let plan = builder.build().unwrap();
        let inspector = Arc::new(FakeInspector {
            environment,
            calls: AtomicUsize::new(0),
            oversized: false,
        });
        let service = ReadService::new(vec![inspector.clone()]);

        let snapshot = service.execute(&plan).await.unwrap();
        assert_eq!(plan.roots.len(), 1);
        assert_eq!(plan.roots[0].consumer_agent_ids.len(), 100);
        assert_eq!(snapshot.facts.len(), 1);
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn scans_scale_by_unique_root_and_one_wsl_plan_is_one_call() {
        let environment = EnvironmentRef::Wsl {
            distro_name: "Ubuntu".to_string(),
        };
        let mut builder = builder(environment.clone());
        for (index, path) in ["/one", "/two", "/three"].into_iter().enumerate() {
            builder
                .add_root(
                    locator(&environment, path),
                    ReadRootPurpose::Detection,
                    Some(AgentId::parse(format!("agent-{index}")).unwrap()),
                )
                .unwrap();
        }
        let plan = builder.build().unwrap();
        let inspector = Arc::new(FakeInspector {
            environment,
            calls: AtomicUsize::new(0),
            oversized: false,
        });
        let service = ReadService::new(vec![inspector.clone()]);

        let snapshot = service.execute(&plan).await.unwrap();
        assert_eq!(plan.roots.len(), 3);
        assert_eq!(snapshot.facts.len(), 3);
        assert_eq!(inspector.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            snapshot.facts[1].error_code.as_deref(),
            Some("pathUnavailable")
        );
        assert_eq!(snapshot.facts[2].kind, FilesystemEntryKind::Directory);
    }

    #[tokio::test]
    async fn rejects_an_inspector_that_exceeds_the_read_plan_byte_contract() {
        let environment = EnvironmentRef::Host;
        let mut builder = builder(environment.clone());
        builder
            .add_root(
                locator(&environment, "/skills"),
                ReadRootPurpose::Canonical,
                None,
            )
            .unwrap();
        let plan = builder.build().unwrap();
        let service = ReadService::new(vec![Arc::new(FakeInspector {
            environment,
            calls: AtomicUsize::new(0),
            oversized: true,
        })]);

        assert!(matches!(
            service.execute(&plan).await,
            Err(AppError::ConfigurationCorrupted { .. })
        ));
    }
}
