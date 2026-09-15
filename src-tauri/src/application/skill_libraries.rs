use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::collection_records::{
    CollectionRecordReader, DocumentRevision, LibraryCatalogRecordReader, SourceRecordRevision,
};
use crate::application::installed_skill_resolver::InstalledSkillResolver;
use crate::application::library_membership::LibraryMembershipOutcome;
use crate::application::library_membership::LibraryMembershipPreview;
use crate::application::mutation::plan::stable_digest;
use crate::application::payload_session::{
    AcquiredPayloadHandle, DiscoverySessionHandle, PayloadSessionManager,
};
use crate::application::skill_changes::{ReadyUpdatePayload, ValidatedSkillPayload};
use crate::application::skill_paths::{
    ContentRevision, ResolvedSkillRoot, ResolvedSkillTarget, SkillPathObserver, SkillTargetRequest,
    TargetRevision,
};
use crate::core::skill::parse_skill_md_content;
use crate::core::skill_payload::{PayloadEntryKind, SkillPayload};
use crate::environment::content_manifest::ContentManifestReader;
use crate::environment::planning::{TargetEntryKind, TargetFactResolver};
use crate::environment::types::{same_environment_identity, EnvironmentRef, SkillLocationRef};
use crate::error::AppError;

pub(crate) const LIBRARY_SCHEMA_VERSION: u32 = 3;

pub type LibraryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct LibraryId(String);

impl LibraryId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub fn parse(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RetirementId(String);

impl RetirementId {
    pub(crate) fn new() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn parse(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillLibrarySummary {
    pub id: LibraryId,
    pub name: String,
    pub skill_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibrarySkillSummary {
    pub name: String,
    pub description: String,
    pub source: String,
    pub source_type: String,
    pub source_url: Option<String>,
    pub skill_path: String,
    pub content_hash: String,
    /// 内容所属插件。属于 Skill 自身的元数据，与 Agent 无关。
    pub plugin_name: Option<String>,
    /// 来源记录中保存的分支或标签。
    pub ref_name: Option<String>,
    /// Skill Deck 最近一次成功提交该成员本地内容的时间，不表示上游发布时间。
    /// 旧成员在下一次成功写入前为 `None`。
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_capability: Option<crate::application::update::CheckUpdateCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillLibraryDetail {
    pub id: LibraryId,
    pub name: String,
    pub skills: Vec<LibrarySkillSummary>,
    pub usages: Vec<LibraryUsage>,
}

/// 某个 Skill 位置引用当前对象的方式。
///
/// `Confirmed` 表示配置已经起作用，`PendingAdjustment` 表示只有未完成的应用操作
/// 引用它、尚未确认生效。两者的并集用于整库删除保护和使用状态展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LibraryUsageState {
    Confirmed,
    PendingAdjustment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUsage {
    pub context: SkillLocationRef,
    pub project: Option<crate::environment::types::RegisteredProject>,
    pub state: LibraryUsageState,
}

/// Skill 库页面用于展示"应用于 N 处"的聚合投影。
///
/// 由一次遍历当前 Environment 全部 Skill 位置得出，读取次数等于位置数量，不随库数量增长。
/// 没有任何位置引用的库不会出现在投影中，调用方按缺失即 0 处理。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryUsageProjection {
    pub library_id: LibraryId,
    pub confirmed_count: u32,
    pub pending_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryUsageSnapshot {
    pub projections: Vec<LibraryUsageProjection>,
    pub inventory_complete: bool,
    pub problem_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct PreviewAddLibrarySkillsRequest {
    pub environment: EnvironmentRef,
    pub library_id: LibraryId,
    pub discovery_session: DiscoverySessionHandle,
    pub skills: Vec<PreviewAddLibrarySkillItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct PreviewAddLibrarySkillItem {
    pub skill_name: String,
    pub payload: AcquiredPayloadHandle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAddSkillRevision {
    pub skill_name: String,
    pub target_revision: String,
    pub source_record_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAddPreviewToken {
    pub generation: String,
    pub context_revision: String,
    pub skill_revisions: Vec<LibraryAddSkillRevision>,
    pub redirected_download_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAddSkillPreview {
    pub skill_name: String,
    pub target_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAddPreview {
    pub token: LibraryAddPreviewToken,
    pub skills: Vec<LibraryAddSkillPreview>,
    pub redirected_download_host: Option<String>,
    pub membership: LibraryMembershipPreview,
}

impl LibraryAddPreview {
    pub(crate) fn bind_membership(
        &mut self,
        request: &PreviewAddLibrarySkillsRequest,
        membership: LibraryMembershipPreview,
    ) -> Result<(), AppError> {
        self.token.generation = library_add_preview_generation(
            request,
            &self.redirected_download_host,
            &self.token.context_revision,
            &self.token.skill_revisions,
            &membership,
        )?;
        self.membership = membership;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ExecuteAddLibrarySkillsRequest {
    pub request: PreviewAddLibrarySkillsRequest,
    pub expected_token: LibraryAddPreviewToken,
    pub membership: LibraryMembershipPreview,
    pub acknowledge_redirect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LibraryAddSkillStatus {
    Succeeded,
    Failed,
    Cancelled,
    NotRun,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAddSkillResult {
    pub skill_name: String,
    pub status: LibraryAddSkillStatus,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryAddResponse {
    pub results: Vec<LibraryAddSkillResult>,
    pub library: Option<SkillLibraryDetail>,
    pub membership: LibraryMembershipOutcome,
}

pub(crate) struct LibraryAddCommitResponse {
    pub(crate) results: Vec<LibraryAddSkillResult>,
    pub(crate) library: Option<SkillLibraryDetail>,
    pub(crate) snapshot_error: Option<AppError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct UpdateLibrarySkillsRequest {
    pub environment: EnvironmentRef,
    pub library_id: LibraryId,
    pub skill_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RemoveLibrarySkillRequest {
    pub environment: EnvironmentRef,
    pub library_id: LibraryId,
    pub skill_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryRetirePreview {
    pub skill_name: String,
    pub token: String,
    pub membership: LibraryMembershipPreview,
}

pub(crate) struct PreparedLibraryRetire {
    skill_name: String,
    base_token: String,
}

impl PreparedLibraryRetire {
    pub(crate) fn skill_name(&self) -> &str {
        &self.skill_name
    }

    pub(crate) fn bind(
        self,
        membership: LibraryMembershipPreview,
    ) -> Result<LibraryRetirePreview, AppError> {
        Ok(LibraryRetirePreview {
            skill_name: self.skill_name,
            token: bound_retire_preview_token(&self.base_token, &membership)?,
            membership,
        })
    }
}

fn bound_retire_preview_token(
    base_token: &str,
    membership: &LibraryMembershipPreview,
) -> Result<String, AppError> {
    stable_digest(&(
        "library-retire-preview",
        base_token,
        membership.token.as_str(),
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ExecuteRetireLibrarySkillRequest {
    pub request: RemoveLibrarySkillRequest,
    pub expected_token: String,
    pub membership: LibraryMembershipPreview,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryRetireResponse {
    pub library: Option<SkillLibraryDetail>,
    pub membership: LibraryMembershipOutcome,
}

pub(crate) struct LibraryRetireCommitResponse {
    pub(crate) library: Option<SkillLibraryDetail>,
    pub(crate) snapshot_error: Option<AppError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryWorkspaceSnapshot {
    pub environment: EnvironmentRef,
    pub libraries: Vec<SkillLibrarySummary>,
    /// catalog 内容的摘要。应用关系不参与该摘要，页面重新进入时自行拉取最新投影。
    pub revision: String,
    pub usage_projection: Vec<LibraryUsageProjection>,
    pub usage_inventory_complete: bool,
    pub usage_inventory_problem_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCatalog {
    pub(crate) schema_version: u32,
    pub(crate) libraries: Vec<SkillLibraryRecord>,
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillLibraryRecord {
    pub(crate) id: LibraryId,
    pub(crate) name: String,
    pub(crate) skills: Vec<LibrarySkillRecord>,
    pub(crate) retired_skills: Vec<RetiredLibrarySkillRecord>,
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetiredLibrarySkillRecord {
    pub(crate) retirement_id: RetirementId,
    pub(crate) member: LibrarySkillRecord,
    pub(crate) retired_at: String,
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibrarySkillRecord {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) source_record: serde_json::Value,
    pub(crate) content_manifest_hash: String,
    /// 与成员目录和来源记录在同一次条件事务中提交。缺失表示该成员写入于本字段引入之前，
    /// 下一次成功写入自然补齐；不根据文件 mtime 推测。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) updated_at: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibrarySkillSourceRecord {
    pub(crate) source_type: String,
    pub(crate) source: String,
    pub(crate) reacquisition_url: Option<String>,
    pub(crate) ref_name: Option<String>,
    pub(crate) skill_path: Option<String>,
    pub(crate) installed_revision: Option<String>,
    pub(crate) computed_hash: Option<String>,
    pub(crate) artifact_url: Option<String>,
    pub(crate) plugin_name: Option<String>,
    pub(crate) well_known: Option<LibraryWellKnownSourceRecord>,
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LibraryWellKnownSourceRecord {
    pub(crate) index_url: String,
    pub(crate) digest: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryMemberCommitExpectation {
    pub(crate) document_revision: DocumentRevision,
    pub(crate) source_record_revision: SourceRecordRevision,
    pub(crate) target_revision: TargetRevision,
    pub(crate) content_revision: ContentRevision,
}

#[derive(Debug, Clone)]
pub(crate) enum LibraryMemberMutation {
    Upsert {
        content: Box<SkillPayload>,
        record: Box<LibrarySkillRecord>,
    },
    Retire {
        retirement_id: RetirementId,
        retired_at: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct CommitLibraryMemberRequest {
    pub(crate) environment: EnvironmentRef,
    pub(crate) library_id: LibraryId,
    pub(crate) skill_name: String,
    pub(crate) expected: LibraryMemberCommitExpectation,
    pub(crate) mutation: LibraryMemberMutation,
}

#[derive(Debug, Clone)]
pub(crate) struct PurgeRetiredLibraryMemberRequest {
    pub(crate) environment: EnvironmentRef,
    pub(crate) library_id: LibraryId,
    pub(crate) skill_name: String,
    pub(crate) retirement_id: RetirementId,
}

impl Default for LibraryCatalog {
    fn default() -> Self {
        Self {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }
}

pub trait SkillLibraryRepository: Send + Sync {
    fn resolve_collection<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<ResolvedSkillRoot, AppError>>;

    fn load<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
    ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>>;

    fn save<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        catalog: &'a LibraryCatalog,
    ) -> LibraryFuture<'a, Result<(), AppError>>;

    fn commit_member<'a>(
        &'a self,
        request: CommitLibraryMemberRequest,
    ) -> LibraryFuture<'a, Result<(), AppError>>;

    fn purge_retired<'a>(
        &'a self,
        request: PurgeRetiredLibraryMemberRequest,
    ) -> LibraryFuture<'a, Result<(), AppError>>;

    fn delete_library<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>>;

    fn read_skill_content<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
        skill_name: &'a str,
    ) -> LibraryFuture<'a, Result<String, AppError>>;
}

pub trait LibraryUsageProvider: Send + Sync {
    /// 返回引用该库的全部 Skill 位置，包含已确认生效和仅被未完成操作引用两种状态。
    ///
    /// 整库删除保护依赖这个并集语义：任何卷入未完成操作的库都不能删除。展示层需要区分
    /// 状态时读取每一项的 `state`，不要改变本方法的收集范围。
    fn usages<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>>;

    /// 一次遍历当前 Environment 的全部 Skill 位置，聚合每个库的使用计数。
    fn usage_projection<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
    ) -> LibraryFuture<'a, Result<LibraryUsageSnapshot, AppError>>;

    fn agent_usages<'a>(
        &'a self,
        _environment: &'a EnvironmentRef,
        _agent_id: &'a crate::core::agent_definition::AgentId,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[cfg(test)]
struct EmptyLibraryUsageProvider;

#[cfg(test)]
impl LibraryUsageProvider for EmptyLibraryUsageProvider {
    fn usages<'a>(
        &'a self,
        _environment: &'a EnvironmentRef,
        _library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn usage_projection<'a>(
        &'a self,
        _environment: &'a EnvironmentRef,
    ) -> LibraryFuture<'a, Result<LibraryUsageSnapshot, AppError>> {
        Box::pin(async {
            Ok(LibraryUsageSnapshot {
                projections: Vec::new(),
                inventory_complete: true,
                problem_count: 0,
            })
        })
    }
}

pub struct SkillLibraryModule {
    repository: Arc<dyn SkillLibraryRepository>,
    usages: Arc<dyn LibraryUsageProvider>,
}

impl SkillLibraryModule {
    pub(crate) fn repository(&self) -> &dyn SkillLibraryRepository {
        self.repository.as_ref()
    }

    #[cfg(test)]
    pub fn new(repository: Arc<dyn SkillLibraryRepository>) -> Self {
        Self {
            repository,
            usages: Arc::new(EmptyLibraryUsageProvider),
        }
    }

    pub fn with_usages(
        repository: Arc<dyn SkillLibraryRepository>,
        usages: Arc<dyn LibraryUsageProvider>,
    ) -> Self {
        Self { repository, usages }
    }

    pub async fn workspace(
        &self,
        environment: EnvironmentRef,
    ) -> Result<LibraryWorkspaceSnapshot, AppError> {
        let catalog = self.repository.load(&environment).await?;
        self.snapshot_with_usages(environment, catalog).await
    }

    /// catalog 与应用投影一起构成页面快照。投影每次重新聚合，因为应用关系可能在
    /// `Skills` 页发生变化，而 catalog 的 revision 不会随之改变。
    async fn snapshot_with_usages(
        &self,
        environment: EnvironmentRef,
        catalog: LibraryCatalog,
    ) -> Result<LibraryWorkspaceSnapshot, AppError> {
        let usage = self.usages.usage_projection(&environment).await?;
        workspace_snapshot(environment, catalog, usage)
    }

    pub async fn create(
        &self,
        environment: EnvironmentRef,
        name: String,
    ) -> Result<LibraryWorkspaceSnapshot, AppError> {
        let name = validated_library_name(name)?;
        let mut catalog = self.repository.load(&environment).await?;
        validate_catalog(&catalog)?;
        ensure_unique_name(&catalog, &environment, &name, None)?;
        catalog.libraries.push(SkillLibraryRecord {
            id: LibraryId(uuid::Uuid::new_v4().simple().to_string()),
            name,
            skills: Vec::new(),
            retired_skills: Vec::new(),
            extra: serde_json::Map::new(),
        });
        self.repository.save(&environment, &catalog).await?;
        self.snapshot_with_usages(environment, catalog).await
    }

    pub async fn rename(
        &self,
        environment: EnvironmentRef,
        id: LibraryId,
        name: String,
    ) -> Result<LibraryWorkspaceSnapshot, AppError> {
        let name = validated_library_name(name)?;
        let mut catalog = self.repository.load(&environment).await?;
        validate_catalog(&catalog)?;
        ensure_unique_name(&catalog, &environment, &name, Some(&id))?;
        let library = catalog
            .libraries
            .iter_mut()
            .find(|library| library.id == id)
            .ok_or_else(|| AppError::PathNotFound {
                path: id.as_str().to_string(),
            })?;
        library.name = name;
        self.repository.save(&environment, &catalog).await?;
        self.snapshot_with_usages(environment, catalog).await
    }

    pub async fn detail(
        &self,
        environment: EnvironmentRef,
        id: LibraryId,
    ) -> Result<SkillLibraryDetail, AppError> {
        let catalog = self.repository.load(&environment).await?;
        let library = catalog
            .libraries
            .into_iter()
            .find(|library| library.id == id)
            .ok_or_else(|| AppError::PathNotFound {
                path: id.as_str().to_string(),
            })?;
        let usages = self.usages.usages(&environment, &id).await?;
        Ok(detail_from_record(library, usages))
    }

    #[cfg(test)]
    pub async fn preview_add_skills<T>(
        &self,
        payloads: &PayloadSessionManager,
        targets: &T,
        request: PreviewAddLibrarySkillsRequest,
        membership: LibraryMembershipPreview,
    ) -> Result<LibraryAddPreview, AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        Ok(self
            .preview_add_skills_with_catalog(payloads, targets, request, membership)
            .await?
            .0)
    }

    pub(crate) async fn preview_add_skills_with_catalog<T>(
        &self,
        payloads: &PayloadSessionManager,
        targets: &T,
        request: PreviewAddLibrarySkillsRequest,
        membership: LibraryMembershipPreview,
    ) -> Result<(LibraryAddPreview, LibraryCatalog), AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        let built = self
            .build_add_skills(payloads, targets, request, membership, true)
            .await?;
        let projected = projected_add_catalog(&built)?;
        Ok((built.preview, projected))
    }

    pub async fn execute_add_skills<T>(
        &self,
        payloads: &PayloadSessionManager,
        targets: &T,
        execution: ExecuteAddLibrarySkillsRequest,
    ) -> Result<LibraryAddCommitResponse, AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        let built = self
            .build_add_skills(
                payloads,
                targets,
                execution.request,
                execution.membership,
                false,
            )
            .await?;
        let expected_generation = library_add_preview_generation(
            &built.request,
            &execution.expected_token.redirected_download_host,
            &execution.expected_token.context_revision,
            &execution.expected_token.skill_revisions,
            &built.preview.membership,
        )?;
        if expected_generation != execution.expected_token.generation {
            return Err(AppError::StaleContext);
        }
        if built.preview.token.context_revision != execution.expected_token.context_revision {
            return Err(AppError::StaleContext);
        }
        if built.preview.redirected_download_host.is_some()
            && built.preview.redirected_download_host
                != execution.expected_token.redirected_download_host
        {
            return Err(AppError::StaleContext);
        }
        if let Some(host) = &execution.expected_token.redirected_download_host {
            if !execution.acknowledge_redirect {
                return Err(AppError::DirectDownloadRedirectConfirmationRequired {
                    host: host.clone(),
                });
            }
        }
        let mut results = Vec::with_capacity(built.items.len());
        let mut items = built.items.into_iter();
        while let Some(item) = items.next() {
            let change = match item {
                BuiltLibraryAddItem::Prepared(change) => *change,
                BuiltLibraryAddItem::Failed(result) => {
                    let cancelled = result.status == LibraryAddSkillStatus::Cancelled;
                    results.push(result);
                    if cancelled {
                        results.extend(items.map(|item| not_run_library_add(item.skill_name())));
                        break;
                    }
                    continue;
                }
            };
            let skill_name = change.skill_name.clone();
            let expected_revision = execution
                .expected_token
                .skill_revisions
                .iter()
                .find(|revision| revision.skill_name == skill_name)
                .ok_or(AppError::StaleContext)?;
            let observed = self
                .observe_add_skill(
                    targets,
                    &built.request.environment,
                    &built.request.library_id,
                    &skill_name,
                    &change.install_dir_name,
                )
                .await;
            let observed = match observed {
                Ok(observed)
                    if observed.context_revision == execution.expected_token.context_revision
                        && observed.target_revision == expected_revision.target_revision
                        && observed.source_record_revision
                            == expected_revision.source_record_revision =>
                {
                    observed
                }
                Ok(_) => {
                    results.push(failed_library_add(&skill_name, AppError::StaleTarget));
                    continue;
                }
                Err(AppError::MutationCancelled) => {
                    results.push(cancelled_library_add(&skill_name));
                    results.extend(items.map(|item| not_run_library_add(item.skill_name())));
                    break;
                }
                Err(error) => {
                    results.push(failed_library_add(&skill_name, error));
                    continue;
                }
            };
            let frontmatter = payload_frontmatter(change.payload.payload())?;
            let record = library_record(&change.payload, frontmatter.description)?;
            let payload = change.payload.payload().clone();
            let result = self
                .repository
                .commit_member(CommitLibraryMemberRequest {
                    environment: built.request.environment.clone(),
                    library_id: built.request.library_id.clone(),
                    skill_name: skill_name.clone(),
                    expected: observed.expected,
                    mutation: LibraryMemberMutation::Upsert {
                        content: Box::new(payload),
                        record: Box::new(record),
                    },
                })
                .await;
            if let Err(error) = result {
                if error == AppError::MutationCancelled {
                    results.push(cancelled_library_add(&skill_name));
                    results.extend(items.map(|item| not_run_library_add(item.skill_name())));
                    break;
                }
                results.push(failed_library_add(&skill_name, error));
                continue;
            }
            results.push(LibraryAddSkillResult {
                skill_name,
                status: LibraryAddSkillStatus::Succeeded,
                error: None,
            });
        }
        let snapshot = self
            .detail(
                built.request.environment.clone(),
                built.request.library_id.clone(),
            )
            .await;
        let (library, snapshot_error) = match snapshot {
            Ok(library) => (Some(library), None),
            Err(error) => (None, Some(error)),
        };
        Ok(LibraryAddCommitResponse {
            results,
            library,
            snapshot_error,
        })
    }

    async fn build_add_skills<T>(
        &self,
        payloads: &PayloadSessionManager,
        targets: &T,
        request: PreviewAddLibrarySkillsRequest,
        membership: LibraryMembershipPreview,
        reject_conflicts: bool,
    ) -> Result<BuiltLibraryAdd, AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        validate_membership_target(&membership, &request.environment, &request.library_id)?;
        if request.skills.is_empty() || request.discovery_session.environment != request.environment
        {
            return Err(AppError::StalePayload);
        }
        let usage_revision = self
            .usage_revision(&request.environment, &request.library_id)
            .await?;
        let catalog = self.repository.load(&request.environment).await?;
        validate_catalog(&catalog)?;
        let library = catalog
            .libraries
            .iter()
            .find(|library| library.id == request.library_id)
            .ok_or_else(|| AppError::PathNotFound {
                path: request.library_id.as_str().to_string(),
            })?;
        let retired = library
            .retired_skills
            .iter()
            .map(|retired| (retired.member.name.as_str(), &retired.member))
            .collect::<std::collections::BTreeMap<_, _>>();
        let collection = self
            .repository
            .resolve_collection(&request.environment, &request.library_id)
            .await?;
        let mut record_names = catalog
            .libraries
            .iter()
            .find(|library| library.id == request.library_id)
            .ok_or_else(|| AppError::PathNotFound {
                path: request.library_id.as_str().to_string(),
            })?
            .skills
            .iter()
            .map(|record| record.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        record_names.extend(request.skills.iter().map(|item| item.skill_name.clone()));
        let records = LibraryCatalogRecordReader::new(&catalog, &request.library_id)
            .load_snapshot(record_names)?;
        let mut items = Vec::with_capacity(request.skills.len());
        if reject_conflicts {
            let mut changes = Vec::with_capacity(request.skills.len());
            for item in &request.skills {
                changes.push(validate_library_add_item(payloads, &request, item).await?);
            }
            let prepared =
                prepare_library_add_targets(targets, &collection, &records, changes, None).await?;
            items.extend(
                prepared
                    .into_iter()
                    .map(|change| BuiltLibraryAddItem::Prepared(Box::new(change))),
            );
        } else {
            for item in &request.skills {
                let prepared = match validate_library_add_item(payloads, &request, item).await {
                    Ok(change) => prepare_library_add_targets(
                        targets,
                        &collection,
                        &records,
                        vec![change],
                        None,
                    )
                    .await
                    .and_then(|mut prepared| prepared.pop().ok_or(AppError::StaleTarget)),
                    Err(error) => Err(error),
                };
                items.push(match prepared {
                    Ok(change) => BuiltLibraryAddItem::Prepared(Box::new(change)),
                    Err(AppError::MutationCancelled) => {
                        BuiltLibraryAddItem::Failed(cancelled_library_add(&item.skill_name))
                    }
                    Err(error) => {
                        BuiltLibraryAddItem::Failed(failed_library_add(&item.skill_name, error))
                    }
                });
            }
        }
        let prepared = items.iter().filter_map(|item| match item {
            BuiltLibraryAddItem::Prepared(change) => Some(change),
            BuiltLibraryAddItem::Failed(_) => None,
        });
        if reject_conflicts {
            for change in prepared.clone() {
                let existing_record = records
                    .records
                    .iter()
                    .find(|record| record.skill_name == change.skill_name)
                    .is_some_and(|record| record.projection.metadata().is_some());
                let reactivating = retired
                    .get(change.skill_name.as_str())
                    .is_some_and(|member| {
                        change.canonical_target.target.entry_kind == TargetEntryKind::Directory
                            && change
                                .canonical_target
                                .content_revision
                                .manifest_hash()
                                .is_some_and(|hash| hash.as_str() == member.content_manifest_hash)
                    });
                if existing_record
                    || (change.canonical_target.target.entry_kind != TargetEntryKind::Missing
                        && !reactivating)
                {
                    return Err(AppError::Validation {
                        field: Some("skillName".to_string()),
                        message: "Skill name already exists in this Library".to_string(),
                    });
                }
            }
        }
        let redirected_download_host = if prepared
            .clone()
            .any(|change| change.payload.source().update.source_type == "download")
        {
            payloads
                .source_snapshot(&request.discovery_session)?
                .descriptor()
                .redirected_download_host
                .clone()
        } else {
            None
        };
        let context_revision = stable_digest(&(
            "library-add-context-v1",
            collection.resolution_revision.as_str(),
            usage_revision,
        ))?;
        let skill_revisions = prepared
            .clone()
            .map(|change| -> Result<LibraryAddSkillRevision, AppError> {
                Ok(LibraryAddSkillRevision {
                    skill_name: change.skill_name.clone(),
                    target_revision: library_target_revision(&change.canonical_target)?,
                    source_record_revision: change
                        .expected_source_record_revision
                        .as_str()
                        .to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let generation = library_add_preview_generation(
            &request,
            &redirected_download_host,
            &context_revision,
            &skill_revisions,
            &membership,
        )?;
        let preview = LibraryAddPreview {
            token: LibraryAddPreviewToken {
                generation,
                context_revision,
                skill_revisions,
                redirected_download_host: redirected_download_host.clone(),
            },
            skills: prepared
                .map(|change| LibraryAddSkillPreview {
                    skill_name: change.skill_name.clone(),
                    target_path: change
                        .canonical_target
                        .target
                        .destination
                        .native_path
                        .clone(),
                })
                .collect(),
            redirected_download_host,
            membership,
        };
        Ok(BuiltLibraryAdd {
            request,
            items,
            preview,
            catalog,
        })
    }

    pub(crate) async fn commit_validated_update<T>(
        &self,
        targets: &T,
        environment: &EnvironmentRef,
        library_id: &LibraryId,
        prepared: ReadyUpdatePayload,
    ) -> Result<(), AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        let current_collection = self
            .repository
            .resolve_collection(environment, library_id)
            .await?;
        if current_collection.resolution_revision != prepared.expected_resolution_revision {
            return Err(AppError::StaleContext);
        }
        let skill_name = prepared.payload.name().to_string();
        let current_target = SkillPathObserver::resolve_skill_targets(
            targets,
            &current_collection,
            vec![SkillTargetRequest {
                skill_name: skill_name.clone(),
            }],
            None,
        )
        .await?
        .into_iter()
        .next()
        .ok_or(AppError::StaleTarget)?;
        if current_target.target_revision != prepared.expected_target_revision
            || current_target.content_revision != prepared.expected_content_revision
        {
            return Err(AppError::StaleTarget);
        }
        let payload = prepared.payload;
        let description = payload_frontmatter(payload.payload())?.description;
        let record = library_record(&payload, description)?;
        let skill_payload = payload.payload().clone();
        let catalog = self.repository.load(environment).await?;
        validate_catalog(&catalog)?;
        let snapshot = LibraryCatalogRecordReader::new(&catalog, library_id)
            .load_snapshot(std::collections::BTreeSet::from([skill_name.clone()]))?;
        let current_record = snapshot.records.first().ok_or(AppError::StaleTarget)?;
        if snapshot.document_revision != prepared.document_revision
            && current_record.source_record_revision != prepared.expected_source_record_revision
        {
            return Err(AppError::StaleTarget);
        }
        self.repository
            .commit_member(CommitLibraryMemberRequest {
                environment: environment.clone(),
                library_id: library_id.clone(),
                skill_name,
                expected: LibraryMemberCommitExpectation {
                    document_revision: snapshot.document_revision,
                    source_record_revision: current_record.source_record_revision.clone(),
                    target_revision: current_target.target_revision,
                    content_revision: current_target.content_revision,
                },
                mutation: LibraryMemberMutation::Upsert {
                    content: Box::new(skill_payload),
                    record: Box::new(record),
                },
            })
            .await
    }

    pub async fn read_skill_content(
        &self,
        environment: EnvironmentRef,
        library_id: LibraryId,
        skill_name: String,
    ) -> Result<String, AppError> {
        let catalog = self.repository.load(&environment).await?;
        let library = catalog
            .libraries
            .iter()
            .find(|library| library.id == library_id)
            .ok_or_else(|| AppError::PathNotFound {
                path: library_id.as_str().to_string(),
            })?;
        if !library.skills.iter().any(|skill| skill.name == skill_name) {
            return Err(AppError::PathNotFound { path: skill_name });
        }
        self.repository
            .read_skill_content(&environment, &library_id, &skill_name)
            .await
    }

    pub(crate) async fn prepare_retire_skill_with_catalog<T>(
        &self,
        targets: &T,
        request: RemoveLibrarySkillRequest,
        membership: LibraryMembershipPreview,
    ) -> Result<(PreparedLibraryRetire, LibraryCatalog), AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        let observed = self
            .observe_retire_skill(targets, &request, &membership)
            .await?;
        let mut projected = observed.catalog;
        crate::application::library_membership::apply_membership_change(
            &mut projected,
            &request.library_id,
            &request.skill_name,
            crate::application::library_membership::MembershipChange::Retire {
                retirement_id: RetirementId("preview".to_string()),
                retired_at: "preview".to_string(),
            },
        )?;
        Ok((
            PreparedLibraryRetire {
                skill_name: request.skill_name,
                base_token: observed.token,
            },
            projected,
        ))
    }

    pub async fn retire_skill<T>(
        &self,
        targets: &T,
        execution: ExecuteRetireLibrarySkillRequest,
    ) -> Result<LibraryRetireCommitResponse, AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        let observed = self
            .observe_retire_skill(targets, &execution.request, &execution.membership)
            .await?;
        if bound_retire_preview_token(&observed.token, &execution.membership)?
            != execution.expected_token
        {
            return Err(AppError::StaleContext);
        }
        let request = execution.request;
        self.repository
            .commit_member(CommitLibraryMemberRequest {
                environment: request.environment.clone(),
                library_id: request.library_id.clone(),
                skill_name: request.skill_name.clone(),
                expected: observed.expected,
                mutation: LibraryMemberMutation::Retire {
                    retirement_id: RetirementId::new(),
                    retired_at: committed_at(),
                },
            })
            .await?;
        let snapshot = self.detail(request.environment, request.library_id).await;
        let (library, snapshot_error) = match snapshot {
            Ok(library) => (Some(library), None),
            Err(error) => (None, Some(error)),
        };
        Ok(LibraryRetireCommitResponse {
            library,
            snapshot_error,
        })
    }

    async fn observe_retire_skill<T>(
        &self,
        targets: &T,
        request: &RemoveLibrarySkillRequest,
        membership: &LibraryMembershipPreview,
    ) -> Result<ObservedLibraryRetire, AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        validate_membership_target(membership, &request.environment, &request.library_id)?;
        let collection = self
            .repository
            .resolve_collection(&request.environment, &request.library_id)
            .await?;
        let catalog = self.repository.load(&request.environment).await?;
        validate_catalog(&catalog)?;
        let snapshot = LibraryCatalogRecordReader::new(&catalog, &request.library_id)
            .load_snapshot(std::collections::BTreeSet::from([request
                .skill_name
                .clone()]))?;
        if matches!(
            snapshot.records[0].projection,
            crate::application::collection_records::RecordProjection::Missing
        ) {
            return Err(AppError::PathNotFound {
                path: request.skill_name.clone(),
            });
        }
        let target = SkillPathObserver::resolve_skill_targets(
            targets,
            &collection,
            vec![SkillTargetRequest {
                skill_name: request.skill_name.clone(),
            }],
            None,
        )
        .await?
        .pop()
        .ok_or(AppError::StaleTarget)?;
        let expected = LibraryMemberCommitExpectation {
            document_revision: snapshot.document_revision,
            source_record_revision: snapshot.records[0].source_record_revision.clone(),
            target_revision: target.target_revision,
            content_revision: target.content_revision,
        };
        let token = stable_digest(&(
            "library-retire-evidence",
            request,
            &expected.document_revision,
            expected.source_record_revision.as_str(),
            &expected.target_revision,
            &expected.content_revision,
        ))?;
        Ok(ObservedLibraryRetire {
            expected,
            token,
            catalog,
        })
    }

    pub async fn delete(
        &self,
        environment: EnvironmentRef,
        library_id: LibraryId,
    ) -> Result<LibraryWorkspaceSnapshot, AppError> {
        let usage = self.ensure_not_applied(&environment, &library_id).await?;
        let catalog = self
            .repository
            .delete_library(&environment, &library_id)
            .await?;
        workspace_snapshot(environment, catalog, usage)
    }

    async fn ensure_not_applied(
        &self,
        environment: &EnvironmentRef,
        library_id: &LibraryId,
    ) -> Result<LibraryUsageSnapshot, AppError> {
        let usage = self.usages.usage_projection(environment).await?;
        if !usage.inventory_complete {
            return Err(AppError::ConfigurationCorrupted {
                message: "Skill Library application inventory is incomplete".to_string(),
            });
        }
        if usage.projections.iter().any(|projection| {
            &projection.library_id == library_id
                && (projection.confirmed_count > 0 || projection.pending_count > 0)
        }) {
            return Err(AppError::Validation {
                field: Some("libraryId".to_string()),
                message: "Skill Library cannot be deleted while it is applied".to_string(),
            });
        }
        Ok(usage)
    }

    async fn usage_revision(
        &self,
        environment: &EnvironmentRef,
        library_id: &LibraryId,
    ) -> Result<String, AppError> {
        let usages = self.usages.usages(environment, library_id).await?;
        stable_digest(&("library-membership-usages-v1", usages))
    }

    async fn observe_add_skill<T>(
        &self,
        targets: &T,
        environment: &EnvironmentRef,
        library_id: &LibraryId,
        skill_name: &str,
        install_dir_name: &str,
    ) -> Result<ObservedLibraryAddSkill, AppError>
    where
        T: TargetFactResolver + ContentManifestReader + ?Sized,
    {
        let usage_revision = self.usage_revision(environment, library_id).await?;
        let collection = self
            .repository
            .resolve_collection(environment, library_id)
            .await?;
        let context_revision = stable_digest(&(
            "library-add-context-v1",
            collection.resolution_revision.as_str(),
            usage_revision,
        ))?;
        let catalog = self.repository.load(environment).await?;
        validate_catalog(&catalog)?;
        if !catalog
            .libraries
            .iter()
            .any(|library| library.id == *library_id)
        {
            return Err(AppError::PathNotFound {
                path: library_id.as_str().to_string(),
            });
        }
        let mut record_names = catalog
            .libraries
            .iter()
            .find(|library| library.id == *library_id)
            .ok_or_else(|| AppError::PathNotFound {
                path: library_id.as_str().to_string(),
            })?
            .skills
            .iter()
            .map(|record| record.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        record_names.insert(skill_name.to_string());
        let records =
            LibraryCatalogRecordReader::new(&catalog, library_id).load_snapshot(record_names)?;
        let source_record_revision = records
            .records
            .iter()
            .find(|record| record.skill_name == skill_name)
            .ok_or(AppError::StaleTarget)?
            .source_record_revision
            .clone();
        let current_skill = skill_name.to_string();
        let mut requests = vec![SkillTargetRequest {
            skill_name: current_skill.clone(),
        }];
        requests.extend(
            records
                .records
                .iter()
                .filter(|record| record.skill_name != skill_name)
                .map(|record| SkillTargetRequest {
                    skill_name: record.skill_name.clone(),
                }),
        );
        let resolved =
            SkillPathObserver::resolve_skill_targets(targets, &collection, requests, None)
                .await?
                .into_iter()
                .find(|resolved| resolved.skill_name == current_skill)
                .ok_or(AppError::StaleTarget)?;
        if resolved.install_dir_name != install_dir_name
            || resolved.target.entry_kind != TargetEntryKind::Missing
        {
            return Err(AppError::StaleTarget);
        }
        Ok(ObservedLibraryAddSkill {
            context_revision,
            target_revision: library_target_revision(&resolved)?,
            source_record_revision: source_record_revision.as_str().to_string(),
            expected: LibraryMemberCommitExpectation {
                document_revision: records.document_revision,
                source_record_revision,
                target_revision: resolved.target_revision,
                content_revision: resolved.content_revision,
            },
        })
    }
}

fn payload_frontmatter(
    payload: &SkillPayload,
) -> Result<crate::core::skill::SkillFrontmatter, AppError> {
    let entry = payload
        .entries
        .iter()
        .find(|entry| {
            entry.kind == PayloadEntryKind::File
                && entry.relative_path.eq_ignore_ascii_case("SKILL.md")
        })
        .ok_or_else(|| AppError::InvalidSkillMd {
            message: "Skill payload is missing SKILL.md".to_string(),
        })?;
    let blob_id = entry.blob_id.as_deref().ok_or(AppError::StalePayload)?;
    let content = payload.blobs.get(blob_id).ok_or(AppError::StalePayload)?;
    let content = std::str::from_utf8(content).map_err(|error| AppError::InvalidSkillMd {
        message: error.to_string(),
    })?;
    parse_skill_md_content(content)
}

fn detail_from_record(
    library: SkillLibraryRecord,
    usages: Vec<LibraryUsage>,
) -> SkillLibraryDetail {
    SkillLibraryDetail {
        id: library.id,
        name: library.name,
        skills: library
            .skills
            .into_iter()
            .map(|skill| {
                let update_metadata =
                    crate::application::collection_records::library_update_metadata(&skill).ok();
                let comparison_fingerprint = update_metadata
                    .as_ref()
                    .map(|metadata| metadata.comparison_fingerprint());
                let update_capability = update_metadata
                    .as_ref()
                    .map(crate::application::update::derive_update_capability_from_metadata);
                let source =
                    serde_json::from_value::<LibrarySkillSourceRecord>(skill.source_record).ok();
                LibrarySkillSummary {
                    name: skill.name,
                    description: skill.description,
                    source: source
                        .as_ref()
                        .map(|value| value.source.clone())
                        .unwrap_or_default(),
                    source_type: source
                        .as_ref()
                        .map(|value| value.source_type.clone())
                        .unwrap_or_default(),
                    source_url: source
                        .as_ref()
                        .and_then(|value| value.reacquisition_url.clone()),
                    skill_path: source
                        .as_ref()
                        .and_then(|value| value.skill_path.clone())
                        .unwrap_or_default(),
                    content_hash: skill.content_manifest_hash,
                    plugin_name: source.as_ref().and_then(|value| value.plugin_name.clone()),
                    ref_name: source.as_ref().and_then(|value| value.ref_name.clone()),
                    updated_at: skill.updated_at,
                    comparison_fingerprint,
                    update_capability,
                }
            })
            .collect(),
        usages,
    }
}

struct BuiltLibraryAdd {
    request: PreviewAddLibrarySkillsRequest,
    items: Vec<BuiltLibraryAddItem>,
    preview: LibraryAddPreview,
    catalog: LibraryCatalog,
}

fn projected_add_catalog(built: &BuiltLibraryAdd) -> Result<LibraryCatalog, AppError> {
    let mut catalog = built.catalog.clone();
    for change in built.items.iter().filter_map(|item| match item {
        BuiltLibraryAddItem::Prepared(change) => Some(change.as_ref()),
        BuiltLibraryAddItem::Failed(_) => None,
    }) {
        let frontmatter = payload_frontmatter(change.payload.payload())?;
        let mut record = library_record(&change.payload, frontmatter.description)?;
        record.updated_at = None;
        crate::application::library_membership::apply_membership_change(
            &mut catalog,
            &built.request.library_id,
            &change.skill_name,
            crate::application::library_membership::MembershipChange::Upsert(record),
        )?;
    }
    Ok(catalog)
}

enum BuiltLibraryAddItem {
    Prepared(Box<PreparedLibraryAdd>),
    Failed(LibraryAddSkillResult),
}

struct PreparedLibraryAdd {
    skill_name: String,
    install_dir_name: String,
    canonical_target: ResolvedSkillTarget,
    expected_source_record_revision: SourceRecordRevision,
    payload: ValidatedSkillPayload,
}

impl BuiltLibraryAddItem {
    fn skill_name(&self) -> &str {
        match self {
            Self::Prepared(change) => &change.skill_name,
            Self::Failed(result) => &result.skill_name,
        }
    }
}

async fn validate_library_add_item(
    payloads: &PayloadSessionManager,
    request: &PreviewAddLibrarySkillsRequest,
    item: &PreviewAddLibrarySkillItem,
) -> Result<ValidatedSkillPayload, AppError> {
    let lease = payloads.pin_verified(&item.payload).await?;
    let payload = ValidatedSkillPayload::validate(
        item.payload.clone(),
        &request.discovery_session,
        &request.environment,
        &item.skill_name,
        lease,
    )
    .await?;
    Ok(payload)
}

async fn prepare_library_add_targets<T>(
    targets: &T,
    root: &ResolvedSkillRoot,
    records: &crate::application::collection_records::CollectionRecordSnapshot,
    payloads: Vec<ValidatedSkillPayload>,
    cancellation: Option<crate::core::mutation::CancellationSignal>,
) -> Result<Vec<PreparedLibraryAdd>, AppError>
where
    T: TargetFactResolver + ContentManifestReader + ?Sized,
{
    if payloads.is_empty() {
        return Err(AppError::Validation {
            field: Some("skills".to_string()),
            message: "at least one Skill is required".to_string(),
        });
    }
    let source_revisions = records
        .records
        .iter()
        .map(|record| {
            (
                record.skill_name.as_str(),
                record.source_record_revision.clone(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let names = payloads
        .iter()
        .map(|payload| payload.name().to_string())
        .collect::<Vec<_>>();
    let observed = SkillPathObserver::resolve_install_targets(
        targets,
        root,
        names,
        records
            .records
            .iter()
            .map(|record| record.skill_name.clone()),
        cancellation,
    )
    .await?;
    payloads
        .into_iter()
        .zip(observed)
        .map(|(payload, canonical_target)| {
            if canonical_target.install_dir_name != payload.install_dir_name() {
                return Err(AppError::StalePayload);
            }
            let skill_name = payload.name().to_string();
            Ok(PreparedLibraryAdd {
                install_dir_name: canonical_target.install_dir_name.clone(),
                expected_source_record_revision: source_revisions
                    .get(skill_name.as_str())
                    .cloned()
                    .ok_or(AppError::StaleContext)?,
                skill_name,
                canonical_target,
                payload,
            })
        })
        .collect()
}

struct ObservedLibraryAddSkill {
    context_revision: String,
    target_revision: String,
    source_record_revision: String,
    expected: LibraryMemberCommitExpectation,
}

struct ObservedLibraryRetire {
    expected: LibraryMemberCommitExpectation,
    token: String,
    catalog: LibraryCatalog,
}

fn library_add_preview_generation(
    request: &PreviewAddLibrarySkillsRequest,
    redirected_download_host: &Option<String>,
    context_revision: &str,
    skill_revisions: &[LibraryAddSkillRevision],
    membership: &LibraryMembershipPreview,
) -> Result<String, AppError> {
    stable_digest(&(
        "library-add-preview-v2",
        request,
        redirected_download_host,
        context_revision,
        skill_revisions,
        membership,
    ))
}

fn library_target_revision(target: &ResolvedSkillTarget) -> Result<String, AppError> {
    stable_digest(&(
        "library-add-target-v1",
        &target.target.key,
        &target.target.fingerprint,
        target
            .content_revision
            .manifest_hash()
            .map(|hash| hash.as_str()),
    ))
}

fn failed_library_add(skill_name: &str, error: AppError) -> LibraryAddSkillResult {
    LibraryAddSkillResult {
        skill_name: skill_name.to_string(),
        status: LibraryAddSkillStatus::Failed,
        error: Some(error),
    }
}

fn cancelled_library_add(skill_name: &str) -> LibraryAddSkillResult {
    LibraryAddSkillResult {
        skill_name: skill_name.to_string(),
        status: LibraryAddSkillStatus::Cancelled,
        error: Some(AppError::MutationCancelled),
    }
}

fn not_run_library_add(skill_name: &str) -> LibraryAddSkillResult {
    LibraryAddSkillResult {
        skill_name: skill_name.to_string(),
        status: LibraryAddSkillStatus::NotRun,
        error: Some(AppError::MutationCancelled),
    }
}

fn library_record(
    payload: &ValidatedSkillPayload,
    description: String,
) -> Result<LibrarySkillRecord, AppError> {
    let source = payload.source();
    Ok(LibrarySkillRecord {
        name: payload.name().to_string(),
        description,
        source_record: serde_json::to_value(library_source_record(source))?,
        content_manifest_hash: payload.content_manifest().to_string(),
        updated_at: Some(committed_at()),
        extra: serde_json::Map::new(),
    })
}

/// 成员提交时间戳。与全局和项目 lock 的 `updatedAt` 使用同一种 UTC RFC 3339 格式。
fn committed_at() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn validate_membership_target(
    membership: &LibraryMembershipPreview,
    environment: &EnvironmentRef,
    library_id: &LibraryId,
) -> Result<(), AppError> {
    if &membership.library_id == library_id
        && same_environment_identity(&membership.environment, environment)
    {
        Ok(())
    } else {
        Err(AppError::StaleContext)
    }
}

fn library_source_record(
    source: &crate::application::skill_changes::NormalizedSkillSource,
) -> LibrarySkillSourceRecord {
    let well_known = (source.update.source_type == "well-known")
        .then(|| {
            source
                .update
                .source_url
                .clone()
                .map(|index_url| LibraryWellKnownSourceRecord {
                    index_url,
                    digest: source.update.well_known_digest.clone(),
                    extra: serde_json::Map::new(),
                })
        })
        .flatten();
    LibrarySkillSourceRecord {
        source_type: source.update.source_type.clone(),
        source: source.update.source.clone(),
        reacquisition_url: (source.update.source_type != "download")
            .then(|| source.update.source_url.clone())
            .flatten(),
        ref_name: source.update.ref_name.clone(),
        skill_path: source.update.skill_path.clone(),
        installed_revision: source.update.remote_hash.clone(),
        computed_hash: source.update.computed_hash.clone(),
        artifact_url: source.artifact_url.clone(),
        plugin_name: source.plugin_name.clone(),
        well_known,
        extra: serde_json::Map::new(),
    }
}

pub(crate) fn merge_unknown_source_fields(
    target: &mut serde_json::Value,
    current: &serde_json::Value,
) {
    let (Some(target), Some(current)) = (target.as_object_mut(), current.as_object()) else {
        return;
    };
    for (key, value) in current {
        target.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

fn workspace_snapshot(
    environment: EnvironmentRef,
    catalog: LibraryCatalog,
    usage: LibraryUsageSnapshot,
) -> Result<LibraryWorkspaceSnapshot, AppError> {
    validate_catalog(&catalog)?;
    let revision = crate::application::mutation::plan::stable_digest(&catalog)?;
    let libraries = catalog
        .libraries
        .into_iter()
        .map(|library| SkillLibrarySummary {
            id: library.id,
            name: library.name,
            skill_count: library.skills.len() as u32,
        })
        .collect();
    Ok(LibraryWorkspaceSnapshot {
        environment,
        libraries,
        revision,
        usage_projection: usage.projections,
        usage_inventory_complete: usage.inventory_complete,
        usage_inventory_problem_count: usage.problem_count,
    })
}

fn validated_library_name(name: String) -> Result<String, AppError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 || name.contains(['\0', '/', '\\']) {
        return Err(AppError::Validation {
            field: Some("libraryName".to_string()),
            message: "Skill Library name must contain 1 to 80 safe characters".to_string(),
        });
    }
    Ok(name)
}

fn ensure_unique_name(
    catalog: &LibraryCatalog,
    environment: &EnvironmentRef,
    name: &str,
    except: Option<&LibraryId>,
) -> Result<(), AppError> {
    let normalized = library_name_key(environment, name);
    if catalog.libraries.iter().any(|library| {
        except != Some(&library.id) && library_name_key(environment, &library.name) == normalized
    }) {
        return Err(AppError::Validation {
            field: Some("libraryName".to_string()),
            message: "Skill Library name already exists".to_string(),
        });
    }
    Ok(())
}

fn library_name_key(environment: &EnvironmentRef, name: &str) -> String {
    let case_insensitive = matches!(environment, EnvironmentRef::Native)
        && (cfg!(target_os = "windows") || cfg!(target_os = "macos"));
    if case_insensitive {
        name.to_lowercase()
    } else {
        name.to_string()
    }
}

pub(crate) fn validate_catalog(catalog: &LibraryCatalog) -> Result<(), AppError> {
    if catalog.schema_version != LIBRARY_SCHEMA_VERSION {
        return Err(AppError::ConfigurationCorrupted {
            message: format!(
                "unsupported Skill Library schema version {}",
                catalog.schema_version
            ),
        });
    }
    let invalid = |message: &str| AppError::ConfigurationCorrupted {
        message: message.to_string(),
    };
    let mut library_ids = std::collections::BTreeSet::new();
    let mut retirement_ids = std::collections::BTreeSet::new();
    for library in &catalog.libraries {
        if !library_ids.insert(library.id.clone())
            || library.name.trim().is_empty()
            || !valid_library_storage_component(library.id.as_str())
        {
            return Err(invalid("invalid Skill Library identity"));
        }
        let mut member_directories = std::collections::BTreeSet::new();
        for member in &library.skills {
            let directory = InstalledSkillResolver::install_dir_name(&member.name)
                .map_err(|_| invalid("invalid active Skill Library member name"))?;
            if !member_directories.insert(directory) || member.content_manifest_hash.is_empty() {
                return Err(invalid("duplicate active Skill Library member directory"));
            }
        }
        for retired in &library.retired_skills {
            let directory = InstalledSkillResolver::install_dir_name(&retired.member.name)
                .map_err(|_| invalid("invalid retired Skill Library member name"))?;
            if retired.retirement_id.as_str().is_empty()
                || retired.retired_at.is_empty()
                || !retirement_ids.insert(retired.retirement_id.as_str())
                || !member_directories.insert(directory)
                || retired.member.content_manifest_hash.is_empty()
            {
                return Err(invalid("invalid retired Skill Library member"));
            }
        }
    }
    Ok(())
}

fn valid_library_storage_component(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "." | "..") && !value.contains(['/', '\\', '\0'])
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::application::payload_session::{PayloadSessionLimits, PayloadSessionManager};
    use crate::core::skill_payload::build_skill_payload;
    use crate::environment::types::EnvironmentKey;

    struct FixedUsages(Vec<LibraryUsage>);

    impl LibraryUsageProvider for FixedUsages {
        fn usages<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
            _library_id: &'a LibraryId,
        ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>> {
            Box::pin(async move { Ok(self.0.clone()) })
        }

        fn usage_projection<'a>(
            &'a self,
            _environment: &'a EnvironmentRef,
        ) -> LibraryFuture<'a, Result<LibraryUsageSnapshot, AppError>> {
            Box::pin(async move {
                let confirmed_count = self
                    .0
                    .iter()
                    .filter(|usage| usage.state == LibraryUsageState::Confirmed)
                    .count() as u32;
                let pending_count = self
                    .0
                    .iter()
                    .filter(|usage| usage.state == LibraryUsageState::PendingAdjustment)
                    .count() as u32;
                Ok(LibraryUsageSnapshot {
                    projections: (!self.0.is_empty())
                        .then(|| LibraryUsageProjection {
                            library_id: LibraryId::parse("library-1"),
                            confirmed_count,
                            pending_count,
                        })
                        .into_iter()
                        .collect(),
                    inventory_complete: true,
                    problem_count: 0,
                })
            })
        }
    }

    #[derive(Default)]
    struct MemoryRepository {
        catalogs: Mutex<HashMap<EnvironmentKey, LibraryCatalog>>,
        payloads: Mutex<HashMap<(EnvironmentKey, String, String), SkillPayload>>,
        roots: Mutex<HashMap<EnvironmentKey, crate::environment::types::ResourceLocator>>,
        write_failures: Mutex<HashMap<String, AppError>>,
    }

    impl SkillLibraryRepository for MemoryRepository {
        fn resolve_collection<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
        ) -> LibraryFuture<'a, Result<ResolvedSkillRoot, AppError>> {
            Box::pin(async move {
                let base = self
                    .roots
                    .lock()
                    .unwrap()
                    .get(&EnvironmentKey::from_ref(environment))
                    .cloned()
                    .ok_or(AppError::StaleEnvironment)?;
                let root = base.join_child(library_id.as_str()).join_child("skills");
                crate::application::skill_paths::SkillPathObserver::resolve_collection(
                    environment.clone(),
                    root,
                    "memory-environment-v1",
                )
            })
        }

        fn load<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
        ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
            Box::pin(async move {
                Ok(self
                    .catalogs
                    .lock()
                    .expect("catalogs")
                    .get(&EnvironmentKey::from_ref(environment))
                    .cloned()
                    .unwrap_or_default())
            })
        }

        fn save<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            catalog: &'a LibraryCatalog,
        ) -> LibraryFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                self.catalogs
                    .lock()
                    .expect("catalogs")
                    .insert(EnvironmentKey::from_ref(environment), catalog.clone());
                Ok(())
            })
        }

        fn commit_member<'a>(
            &'a self,
            request: CommitLibraryMemberRequest,
        ) -> LibraryFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                let environment_key = EnvironmentKey::from_ref(&request.environment);
                let mut catalogs = self.catalogs.lock().expect("catalogs");
                let catalog = catalogs.entry(environment_key.clone()).or_default();
                let snapshot = LibraryCatalogRecordReader::new(catalog, &request.library_id)
                    .load_snapshot(std::collections::BTreeSet::from([request
                        .skill_name
                        .clone()]))?;
                let current = snapshot.records.first().ok_or(AppError::StaleTarget)?;
                if current.source_record_revision != request.expected.source_record_revision {
                    return Err(AppError::StaleTarget);
                }
                let _ = (
                    request.expected.document_revision,
                    request.expected.target_revision,
                    request.expected.content_revision,
                );
                if let Some(error) = self
                    .write_failures
                    .lock()
                    .expect("write failures")
                    .get(&request.skill_name)
                    .cloned()
                {
                    return Err(error);
                }
                match request.mutation {
                    LibraryMemberMutation::Upsert { content, record } => {
                        crate::application::library_membership::apply_membership_change(
                            catalog,
                            &request.library_id,
                            &request.skill_name,
                            crate::application::library_membership::MembershipChange::Upsert(
                                *record,
                            ),
                        )?;
                        self.payloads.lock().expect("payloads").insert(
                            (
                                environment_key,
                                request.library_id.as_str().to_string(),
                                request.skill_name,
                            ),
                            *content,
                        );
                    }
                    LibraryMemberMutation::Retire {
                        retirement_id,
                        retired_at,
                    } => {
                        crate::application::library_membership::apply_membership_change(
                            catalog,
                            &request.library_id,
                            &request.skill_name,
                            crate::application::library_membership::MembershipChange::Retire {
                                retirement_id,
                                retired_at,
                            },
                        )?;
                    }
                }
                Ok(())
            })
        }

        fn purge_retired<'a>(
            &'a self,
            request: PurgeRetiredLibraryMemberRequest,
        ) -> LibraryFuture<'a, Result<(), AppError>> {
            Box::pin(async move {
                let environment_key = EnvironmentKey::from_ref(&request.environment);
                let mut catalogs = self.catalogs.lock().expect("catalogs");
                let catalog = catalogs.entry(environment_key.clone()).or_default();
                crate::application::library_membership::apply_membership_change(
                    catalog,
                    &request.library_id,
                    &request.skill_name,
                    crate::application::library_membership::MembershipChange::Purge {
                        retirement_id: request.retirement_id,
                    },
                )?;
                self.payloads.lock().expect("payloads").remove(&(
                    environment_key,
                    request.library_id.as_str().to_string(),
                    request.skill_name,
                ));
                Ok(())
            })
        }

        fn delete_library<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
        ) -> LibraryFuture<'a, Result<LibraryCatalog, AppError>> {
            Box::pin(async move {
                let environment_key = EnvironmentKey::from_ref(environment);
                let library_id = library_id.as_str();
                let mut catalogs = self.catalogs.lock().expect("catalogs");
                let catalog = catalogs.entry(environment_key.clone()).or_default();
                let before = catalog.libraries.len();
                catalog
                    .libraries
                    .retain(|library| library.id.as_str() != library_id);
                if catalog.libraries.len() == before {
                    return Err(AppError::PathNotFound {
                        path: library_id.to_string(),
                    });
                }
                self.payloads.lock().expect("payloads").retain(
                    |(candidate_environment, candidate_library, _), _| {
                        candidate_environment != &environment_key || candidate_library != library_id
                    },
                );
                Ok(catalog.clone())
            })
        }

        fn read_skill_content<'a>(
            &'a self,
            environment: &'a EnvironmentRef,
            library_id: &'a LibraryId,
            skill_name: &'a str,
        ) -> LibraryFuture<'a, Result<String, AppError>> {
            Box::pin(async move {
                let payload = self
                    .payloads
                    .lock()
                    .expect("payloads")
                    .get(&(
                        EnvironmentKey::from_ref(environment),
                        library_id.as_str().to_string(),
                        skill_name.to_string(),
                    ))
                    .cloned()
                    .ok_or_else(|| AppError::PathNotFound {
                        path: skill_name.to_string(),
                    })?;
                let entry = payload
                    .entries
                    .iter()
                    .find(|entry| entry.relative_path.eq_ignore_ascii_case("SKILL.md"))
                    .ok_or_else(|| AppError::InvalidSkillMd {
                        message: "Skill payload is missing SKILL.md".to_string(),
                    })?;
                let bytes = payload
                    .blobs
                    .get(entry.blob_id.as_deref().ok_or(AppError::StalePayload)?)
                    .ok_or(AppError::StalePayload)?;
                let markdown =
                    std::str::from_utf8(bytes).map_err(|error| AppError::InvalidSkillMd {
                        message: error.to_string(),
                    })?;
                Ok(crate::core::skill::skill_content_from_markdown(markdown))
            })
        }
    }

    async fn acquired_item(
        manager: &PayloadSessionManager,
        discovery: &DiscoverySessionHandle,
        root: &std::path::Path,
        name: &str,
    ) -> PreviewAddLibrarySkillItem {
        let source = root.join(name);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name}\n---\nBody\n"),
        )
        .unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let skill_path = format!("skills/{name}");
        let payload = manager
            .acquire_payload_with_metadata(
                discovery,
                &skill_path,
                payload,
                crate::application::payload_session::PayloadPlanningMetadata {
                    skill_name: name.to_string(),
                    install_dir_name: InstalledSkillResolver::install_dir_name(name).unwrap(),
                    source: "https://example.com/repo.git".to_string(),
                    source_type: "git".to_string(),
                    source_url: Some("https://example.com/repo".to_string()),
                    ref_name: None,
                    skill_path: skill_path.clone(),
                    plugin_name: None,
                    computed_hash: format!("hash-{name}"),
                    upstream_revision: Some(format!("tree-{name}")),
                    well_known: None,
                },
            )
            .await
            .unwrap();
        PreviewAddLibrarySkillItem {
            skill_name: name.to_string(),
            payload,
        }
    }

    fn payload_manager() -> PayloadSessionManager {
        payload_manager_with_storage().0
    }

    fn membership_preview(
        environment: &EnvironmentRef,
        library_id: &LibraryId,
    ) -> LibraryMembershipPreview {
        LibraryMembershipPreview {
            environment: environment.clone(),
            library_id: library_id.clone(),
            scopes: Vec::new(),
            impacts: Vec::new(),
            inventory_complete: true,
            inventory_token: "membership-preview".to_string(),
            token: "membership-preview".to_string(),
        }
    }

    fn payload_manager_with_storage() -> (
        PayloadSessionManager,
        Arc<crate::application::payload_session::InMemoryPayloadSessionStorage>,
    ) {
        let storage =
            Arc::new(crate::application::payload_session::InMemoryPayloadSessionStorage::default());
        let manager = PayloadSessionManager::new(
            storage.clone(),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        );
        (manager, storage)
    }

    #[test]
    fn membership_preview_must_match_the_member_request_target() {
        let environment = EnvironmentRef::Native;
        let library_id = LibraryId::parse("library-1");
        let preview = membership_preview(&environment, &LibraryId::parse("library-2"));

        assert!(matches!(
            validate_membership_target(&preview, &environment, &library_id),
            Err(AppError::StaleContext)
        ));
    }

    #[test]
    fn retire_preview_token_binds_the_final_membership_impact() {
        let environment = EnvironmentRef::Native;
        let library_id = LibraryId::parse("library-1");
        let prepared = PreparedLibraryRetire {
            skill_name: "demo".to_string(),
            base_token: "member-evidence".to_string(),
        };
        let mut final_membership = membership_preview(&environment, &library_id);
        final_membership.token = "impact-evidence".to_string();

        let preview = prepared.bind(final_membership).unwrap();

        assert_ne!(preview.token, "member-evidence");
        assert_eq!(
            preview.token,
            bound_retire_preview_token("member-evidence", &preview.membership).unwrap()
        );
    }

    #[tokio::test]
    async fn creates_lists_and_renames_a_library_without_changing_its_id() {
        let module = SkillLibraryModule::new(Arc::new(MemoryRepository::default()));
        let environment = EnvironmentRef::Native;

        let created = module
            .create(environment.clone(), "Java Backend".to_string())
            .await
            .expect("create");
        let id = created.libraries[0].id.clone();
        assert_eq!(created.libraries[0].name, "Java Backend");
        assert_eq!(created.libraries[0].skill_count, 0);

        let renamed = module
            .rename(environment.clone(), id.clone(), "Backend".to_string())
            .await
            .expect("rename");
        assert_eq!(renamed.libraries[0].id, id);
        assert_eq!(renamed.libraries[0].name, "Backend");
        assert_eq!(
            module.workspace(environment.clone()).await.unwrap(),
            renamed
        );

        let deleted = module
            .delete(environment, id)
            .await
            .expect("delete empty library");
        assert!(deleted.libraries.is_empty());
    }

    #[test]
    fn catalog_round_trip_preserves_unknown_fields_at_every_record_level() {
        let value = serde_json::json!({
            "schemaVersion": LIBRARY_SCHEMA_VERSION,
            "catalogExtension": { "enabled": true },
            "libraries": [{
                "id": "library-1",
                "name": "Backend",
                "libraryExtension": 1,
                "skills": [{
                    "name": "demo",
                    "description": "Demo",
                    "skillExtension": "keep",
                    "sourceRecord": {
                        "sourceType": "git",
                        "source": "https://example.com/repo.git",
                        "reacquisitionUrl": "https://example.com/repo.git",
                        "refName": "main",
                        "skillPath": "skills/demo",
                        "installedRevision": "old",
                        "computedHash": "old",
                        "artifactUrl": null,
                        "pluginName": null,
                        "wellKnown": null,
                        "sourceExtension": ["keep"]
                    },
                    "contentManifestHash": "manifest-old"
                }],
                "retiredSkills": []
            }]
        });

        let catalog: LibraryCatalog = serde_json::from_value(value).unwrap();
        let serialized = serde_json::to_value(catalog).unwrap();

        assert_eq!(serialized["catalogExtension"]["enabled"], true);
        assert_eq!(serialized["libraries"][0]["libraryExtension"], 1);
        assert_eq!(
            serialized["libraries"][0]["skills"][0]["skillExtension"],
            "keep"
        );
        assert_eq!(
            serialized["libraries"][0]["skills"][0]["sourceRecord"]["sourceExtension"][0],
            "keep"
        );
    }

    #[test]
    fn catalog_rejects_active_and_retired_directory_collision() {
        let active = test_library_record("CE:Review", "active");
        let retired = test_library_record("ce-review", "retired");
        let catalog = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: LibraryId::parse("library-1"),
                name: "Library".to_string(),
                skills: vec![active],
                retired_skills: vec![RetiredLibrarySkillRecord {
                    retirement_id: RetirementId::parse("retirement-1"),
                    member: retired,
                    retired_at: "2026-09-06T00:00:00Z".to_string(),
                    extra: serde_json::Map::new(),
                }],
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };

        assert!(matches!(
            validate_catalog(&catalog),
            Err(AppError::ConfigurationCorrupted { .. })
        ));
    }

    #[test]
    fn library_update_request_contains_intent_instead_of_payload_handles() {
        let request = UpdateLibrarySkillsRequest {
            environment: EnvironmentRef::Native,
            library_id: LibraryId::parse("library-1"),
            skill_names: vec!["api-design".to_string()],
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "environment": { "kind": "native" },
                "libraryId": "library-1",
                "skillNames": ["api-design"]
            })
        );
    }

    #[test]
    fn catalog_source_records_keep_every_supported_reacquisition_field() {
        for source_type in ["github", "gitlab", "git", "local", "well-known", "download"] {
            let source = crate::application::skill_changes::NormalizedSkillSource {
                update: crate::core::NormalizedUpdateMetadata {
                    source: "source-identity".to_string(),
                    source_type: source_type.to_string(),
                    source_url: Some("https://example.com/reacquire".to_string()),
                    ref_name: Some("main".to_string()),
                    skill_path: Some("skills/demo".to_string()),
                    remote_hash: Some("revision-v1".to_string()),
                    computed_hash: Some("computed-v1".to_string()),
                    well_known_digest: Some("sha256:index-v1".to_string()),
                },
                artifact_url: Some("https://cdn.example.com/demo.tar.gz".to_string()),
                plugin_name: Some("plugin-demo".to_string()),
            };

            let record = library_source_record(&source);

            assert_eq!(record.source_type, source_type);
            assert_eq!(record.source, "source-identity");
            assert_eq!(
                record.reacquisition_url.as_deref(),
                (source_type != "download").then_some("https://example.com/reacquire")
            );
            assert_eq!(record.ref_name.as_deref(), Some("main"));
            assert_eq!(record.skill_path.as_deref(), Some("skills/demo"));
            assert_eq!(record.installed_revision.as_deref(), Some("revision-v1"));
            assert_eq!(record.computed_hash.as_deref(), Some("computed-v1"));
            assert_eq!(
                record.artifact_url.as_deref(),
                Some("https://cdn.example.com/demo.tar.gz")
            );
            assert_eq!(
                record
                    .well_known
                    .as_ref()
                    .and_then(|value| value.digest.as_deref()),
                (source_type == "well-known").then_some("sha256:index-v1")
            );
            assert_eq!(record.plugin_name.as_deref(), Some("plugin-demo"));
        }
    }

    #[tokio::test]
    async fn conditional_member_commit_merges_unrelated_catalog_changes() {
        let repository = MemoryRepository::default();
        let environment = EnvironmentRef::Native;
        let library_id = LibraryId::parse("library-1");
        let initial = LibraryCatalog {
            schema_version: LIBRARY_SCHEMA_VERSION,
            libraries: vec![SkillLibraryRecord {
                id: library_id.clone(),
                name: "Library".to_string(),
                skills: vec![
                    test_library_record("alpha", "alpha-v1"),
                    test_library_record("beta", "beta-v1"),
                ],
                retired_skills: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            extra: serde_json::Map::new(),
        };
        repository.save(&environment, &initial).await.unwrap();
        let snapshot = LibraryCatalogRecordReader::new(&initial, &library_id)
            .load_snapshot(std::collections::BTreeSet::from(["alpha".to_string()]))
            .unwrap();
        let mut concurrent = initial.clone();
        concurrent.libraries[0].skills[1] = test_library_record("beta", "beta-v2");
        repository.save(&environment, &concurrent).await.unwrap();

        repository
            .commit_member(CommitLibraryMemberRequest {
                environment: environment.clone(),
                library_id: library_id.clone(),
                skill_name: "alpha".to_string(),
                expected: LibraryMemberCommitExpectation {
                    document_revision: snapshot.document_revision,
                    source_record_revision: snapshot.records[0].source_record_revision.clone(),
                    target_revision: TargetRevision::for_test("target-alpha"),
                    content_revision: ContentRevision::missing_for_test(),
                },
                mutation: LibraryMemberMutation::Upsert {
                    content: Box::new(build_skill_payload_for_test("alpha", "alpha-v2")),
                    record: Box::new(test_library_record("alpha", "alpha-v2")),
                },
            })
            .await
            .unwrap();

        let saved = repository.load(&environment).await.unwrap();
        assert_eq!(source_revision(&saved.libraries[0].skills[0]), "alpha-v2");
        assert_eq!(source_revision(&saved.libraries[0].skills[1]), "beta-v2");
    }

    fn test_library_record(name: &str, revision: &str) -> LibrarySkillRecord {
        LibrarySkillRecord {
            name: name.to_string(),
            description: name.to_string(),
            source_record: serde_json::json!({
                "sourceType": "git",
                "source": "https://example.com/repo.git",
                "reacquisitionUrl": "https://example.com/repo.git",
                "refName": "main",
                "skillPath": format!("skills/{name}"),
                "installedRevision": revision,
                "computedHash": revision,
                "pluginName": null,
                "artifactUrl": null,
                "wellKnown": null
            }),
            content_manifest_hash: format!("manifest-{revision}"),
            updated_at: None,
            extra: serde_json::Map::new(),
        }
    }

    fn source_revision(record: &LibrarySkillRecord) -> String {
        record.source_record["installedRevision"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn build_skill_payload_for_test(name: &str, revision: &str) -> SkillPayload {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {revision}\n---\n{revision}\n"),
        )
        .unwrap();
        build_skill_payload(root.path()).unwrap()
    }

    #[tokio::test]
    async fn applied_library_blocks_membership_changes_and_deletion() {
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: crate::environment::types::SkillLocation::Global,
        };
        let module = SkillLibraryModule::with_usages(
            Arc::new(MemoryRepository::default()),
            Arc::new(FixedUsages(vec![LibraryUsage {
                context,
                project: None,
                state: LibraryUsageState::Confirmed,
            }])),
        );
        let id = LibraryId::parse("library-1");

        assert!(matches!(
            module.ensure_not_applied(&EnvironmentRef::Native, &id).await,
            Err(AppError::Validation {
                field: Some(field),
                ..
            }) if field == "libraryId"
        ));
    }

    #[test]
    fn a_member_written_before_updated_at_reads_back_as_none_and_stays_omitted() {
        let raw = serde_json::json!({
            "name": "api-design",
            "description": "Design APIs",
            "sourceRecord": { "sourceType": "git", "source": "https://example.com/repo.git" },
            "contentManifestHash": "manifest-1",
        });

        let record: LibrarySkillRecord = serde_json::from_value(raw).unwrap();

        assert_eq!(record.updated_at, None);
        // 不写回空字段，未触碰的旧成员在 catalog 中保持原样。
        let round_trip = serde_json::to_value(&record).unwrap();
        assert!(round_trip.get("updatedAt").is_none());
    }

    #[tokio::test]
    async fn a_pending_only_usage_still_blocks_membership_changes() {
        // 未完成的应用操作同样锁定成员：并集语义不能因为区分展示状态而变化。
        let context = SkillLocationRef {
            environment: EnvironmentRef::Native,
            scope: crate::environment::types::SkillLocation::Global,
        };
        let module = SkillLibraryModule::with_usages(
            Arc::new(MemoryRepository::default()),
            Arc::new(FixedUsages(vec![LibraryUsage {
                context,
                project: None,
                state: LibraryUsageState::PendingAdjustment,
            }])),
        );

        assert!(module
            .ensure_not_applied(&EnvironmentRef::Native, &LibraryId::parse("library-1"))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn one_stale_target_does_not_stop_other_library_adds() {
        let repository = Arc::new(MemoryRepository::default());
        let module = SkillLibraryModule::new(repository.clone());
        let environment = EnvironmentRef::Native;
        let library_id = module
            .create(environment.clone(), "Backend".to_string())
            .await
            .unwrap()
            .libraries[0]
            .id
            .clone();
        let temp = tempfile::tempdir().unwrap();
        let library_root = temp.path().join("libraries");
        let skills_root = library_root.join(library_id.as_str()).join("skills");
        std::fs::create_dir_all(&skills_root).unwrap();
        repository.roots.lock().unwrap().insert(
            EnvironmentKey::from_ref(&environment),
            crate::environment::types::ResourceLocator {
                environment: environment.clone(),
                native_path: library_root.to_string_lossy().into_owned(),
            },
        );
        let manager = payload_manager();
        let discovery = manager
            .discover(environment.clone(), "https://example.com/repo.git")
            .await
            .unwrap();
        let request = PreviewAddLibrarySkillsRequest {
            environment: environment.clone(),
            library_id: library_id.clone(),
            discovery_session: discovery.clone(),
            skills: vec![
                acquired_item(&manager, &discovery, temp.path(), "stale").await,
                acquired_item(&manager, &discovery, temp.path(), "ready").await,
            ],
        };
        let targets = crate::environment::planning::RuntimeTargetFactResolver::new(Arc::new(
            crate::environment::wsl::WslRuntime::default(),
        ));
        let preview = module
            .preview_add_skills(
                &manager,
                &targets,
                request.clone(),
                membership_preview(&request.environment, &request.library_id),
            )
            .await
            .unwrap();
        std::fs::create_dir_all(skills_root.join("stale")).unwrap();

        let response = module
            .execute_add_skills(
                &manager,
                &targets,
                ExecuteAddLibrarySkillsRequest {
                    request,
                    expected_token: preview.token.clone(),
                    membership: preview.membership.clone(),
                    acknowledge_redirect: false,
                },
            )
            .await
            .unwrap();

        assert_eq!(response.results[0].status, LibraryAddSkillStatus::Failed);
        assert_eq!(response.results[0].error, Some(AppError::StaleTarget));
        assert_eq!(response.results[1].status, LibraryAddSkillStatus::Succeeded);
        assert_eq!(response.library.as_ref().unwrap().skills[0].name, "ready");
    }

    #[tokio::test]
    async fn adds_multiple_fresh_library_skills_from_one_preview() {
        let repository = Arc::new(MemoryRepository::default());
        let module = SkillLibraryModule::new(repository.clone());
        let environment = EnvironmentRef::Native;
        let library_id = module
            .create(environment.clone(), "Writing".to_string())
            .await
            .unwrap()
            .libraries[0]
            .id
            .clone();
        let temp = tempfile::tempdir().unwrap();
        let library_root = temp.path().join("libraries");
        std::fs::create_dir_all(library_root.join(library_id.as_str()).join("skills")).unwrap();
        repository.roots.lock().unwrap().insert(
            EnvironmentKey::from_ref(&environment),
            crate::environment::types::ResourceLocator {
                environment: environment.clone(),
                native_path: library_root.to_string_lossy().into_owned(),
            },
        );
        let manager = payload_manager();
        let discovery = manager
            .discover(environment.clone(), "https://example.com/repo.git")
            .await
            .unwrap();
        let request = PreviewAddLibrarySkillsRequest {
            environment: environment.clone(),
            library_id,
            discovery_session: discovery.clone(),
            skills: vec![
                acquired_item(&manager, &discovery, temp.path(), "alpha").await,
                acquired_item(&manager, &discovery, temp.path(), "beta").await,
            ],
        };
        let targets = crate::environment::planning::RuntimeTargetFactResolver::new(Arc::new(
            crate::environment::wsl::WslRuntime::default(),
        ));
        let preview = module
            .preview_add_skills(
                &manager,
                &targets,
                request.clone(),
                membership_preview(&request.environment, &request.library_id),
            )
            .await
            .unwrap();

        let response = module
            .execute_add_skills(
                &manager,
                &targets,
                ExecuteAddLibrarySkillsRequest {
                    request,
                    expected_token: preview.token.clone(),
                    membership: preview.membership.clone(),
                    acknowledge_redirect: false,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            response
                .results
                .iter()
                .map(|result| (result.skill_name.as_str(), result.status))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", LibraryAddSkillStatus::Succeeded),
                ("beta", LibraryAddSkillStatus::Succeeded),
            ]
        );
        assert_eq!(
            response
                .library
                .as_ref()
                .unwrap()
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
    }

    #[tokio::test]
    async fn one_stale_payload_does_not_stop_other_library_adds() {
        use crate::application::payload_session::{PayloadSessionStorage, PayloadStorageKey};

        let repository = Arc::new(MemoryRepository::default());
        let module = SkillLibraryModule::new(repository.clone());
        let environment = EnvironmentRef::Native;
        let library_id = module
            .create(environment.clone(), "Backend".to_string())
            .await
            .unwrap()
            .libraries[0]
            .id
            .clone();
        let temp = tempfile::tempdir().unwrap();
        let library_root = temp.path().join("libraries");
        std::fs::create_dir_all(library_root.join(library_id.as_str()).join("skills")).unwrap();
        repository.roots.lock().unwrap().insert(
            EnvironmentKey::from_ref(&environment),
            crate::environment::types::ResourceLocator {
                environment: environment.clone(),
                native_path: library_root.to_string_lossy().into_owned(),
            },
        );
        let (manager, storage) = payload_manager_with_storage();
        let discovery = manager
            .discover(environment.clone(), "https://example.com/repo.git")
            .await
            .unwrap();
        let request = PreviewAddLibrarySkillsRequest {
            environment: environment.clone(),
            library_id: library_id.clone(),
            discovery_session: discovery.clone(),
            skills: vec![
                acquired_item(&manager, &discovery, temp.path(), "stale").await,
                acquired_item(&manager, &discovery, temp.path(), "ready").await,
            ],
        };
        let targets = crate::environment::planning::RuntimeTargetFactResolver::new(Arc::new(
            crate::environment::wsl::WslRuntime::default(),
        ));
        let preview = module
            .preview_add_skills(
                &manager,
                &targets,
                request.clone(),
                membership_preview(&request.environment, &request.library_id),
            )
            .await
            .unwrap();
        let stale = &request.skills[0].payload;
        storage
            .remove(&PayloadStorageKey::new(
                &stale.session_id,
                &stale.skill_path,
            ))
            .await
            .unwrap();

        let response = module
            .execute_add_skills(
                &manager,
                &targets,
                ExecuteAddLibrarySkillsRequest {
                    request,
                    expected_token: preview.token.clone(),
                    membership: preview.membership.clone(),
                    acknowledge_redirect: false,
                },
            )
            .await
            .unwrap();

        assert_eq!(response.results[0].status, LibraryAddSkillStatus::Failed);
        assert_eq!(response.results[0].error, Some(AppError::StalePayload));
        assert_eq!(response.results[1].status, LibraryAddSkillStatus::Succeeded);
        assert_eq!(response.library.as_ref().unwrap().skills[0].name, "ready");
    }

    #[tokio::test]
    async fn cancellation_marks_the_current_and_remaining_library_adds() {
        let repository = Arc::new(MemoryRepository::default());
        let module = SkillLibraryModule::new(repository.clone());
        let environment = EnvironmentRef::Native;
        let library_id = module
            .create(environment.clone(), "Backend".to_string())
            .await
            .unwrap()
            .libraries[0]
            .id
            .clone();
        let temp = tempfile::tempdir().unwrap();
        let library_root = temp.path().join("libraries");
        std::fs::create_dir_all(library_root.join(library_id.as_str()).join("skills")).unwrap();
        repository.roots.lock().unwrap().insert(
            EnvironmentKey::from_ref(&environment),
            crate::environment::types::ResourceLocator {
                environment: environment.clone(),
                native_path: library_root.to_string_lossy().into_owned(),
            },
        );
        repository
            .write_failures
            .lock()
            .unwrap()
            .insert("cancelled".to_string(), AppError::MutationCancelled);
        let manager = payload_manager();
        let discovery = manager
            .discover(environment.clone(), "https://example.com/repo.git")
            .await
            .unwrap();
        let request = PreviewAddLibrarySkillsRequest {
            environment: environment.clone(),
            library_id: library_id.clone(),
            discovery_session: discovery.clone(),
            skills: vec![
                acquired_item(&manager, &discovery, temp.path(), "cancelled").await,
                acquired_item(&manager, &discovery, temp.path(), "later").await,
            ],
        };
        let targets = crate::environment::planning::RuntimeTargetFactResolver::new(Arc::new(
            crate::environment::wsl::WslRuntime::default(),
        ));
        let preview = module
            .preview_add_skills(
                &manager,
                &targets,
                request.clone(),
                membership_preview(&request.environment, &request.library_id),
            )
            .await
            .unwrap();

        let response = module
            .execute_add_skills(
                &manager,
                &targets,
                ExecuteAddLibrarySkillsRequest {
                    request,
                    expected_token: preview.token.clone(),
                    membership: preview.membership.clone(),
                    acknowledge_redirect: false,
                },
            )
            .await
            .unwrap();

        assert_eq!(response.results[0].status, LibraryAddSkillStatus::Cancelled);
        assert_eq!(response.results[1].status, LibraryAddSkillStatus::NotRun);
        assert!(response.library.as_ref().unwrap().skills.is_empty());
    }

    #[tokio::test]
    async fn adds_an_acquired_skill_with_independent_source_metadata() {
        let repository = Arc::new(MemoryRepository::default());
        let module = SkillLibraryModule::new(repository.clone());
        let environment = EnvironmentRef::Native;
        let created = module
            .create(environment.clone(), "Backend".to_string())
            .await
            .unwrap();
        let library_id = created.libraries[0].id.clone();
        let temp = tempfile::tempdir().unwrap();
        let library_root = temp.path().join("libraries");
        std::fs::create_dir_all(library_root.join(library_id.as_str()).join("skills")).unwrap();
        repository.roots.lock().unwrap().insert(
            EnvironmentKey::from_ref(&environment),
            crate::environment::types::ResourceLocator {
                environment: environment.clone(),
                native_path: library_root.to_string_lossy().into_owned(),
            },
        );
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            b"---\nname: api-design\ndescription: Design APIs\n---\nBody\n",
        )
        .unwrap();
        let payload = build_skill_payload(&source).unwrap();
        let manager = PayloadSessionManager::new(
            Arc::new(crate::application::payload_session::InMemoryPayloadSessionStorage::default()),
            PayloadSessionLimits {
                ttl_ms: 60_000,
                max_sessions: 4,
                max_bytes: 1_000_000,
            },
            || 1_000,
        );
        let discovery = manager
            .discover(environment.clone(), "https://example.com/repo.git")
            .await
            .unwrap();
        let handle = manager
            .acquire_payload_with_metadata(
                &discovery,
                "skills/api-design",
                payload,
                crate::application::payload_session::PayloadPlanningMetadata {
                    skill_name: "api-design".to_string(),
                    install_dir_name: "api-design".to_string(),
                    source: "https://example.com/repo.git".to_string(),
                    source_type: "git".to_string(),
                    source_url: Some("https://example.com/repo".to_string()),
                    ref_name: None,
                    skill_path: "skills/api-design".to_string(),
                    plugin_name: None,
                    computed_hash: "hash-v1".to_string(),
                    upstream_revision: Some("tree-v1".to_string()),
                    well_known: None,
                },
            )
            .await
            .unwrap();

        let request = PreviewAddLibrarySkillsRequest {
            environment: environment.clone(),
            library_id: library_id.clone(),
            discovery_session: discovery,
            skills: vec![PreviewAddLibrarySkillItem {
                skill_name: "api-design".to_string(),
                payload: handle,
            }],
        };
        let targets = crate::environment::planning::RuntimeTargetFactResolver::new(Arc::new(
            crate::environment::wsl::WslRuntime::default(),
        ));
        let preview = module
            .preview_add_skills(
                &manager,
                &targets,
                request.clone(),
                membership_preview(&request.environment, &request.library_id),
            )
            .await
            .expect("preview add skill");
        assert!(repository.payloads.lock().unwrap().is_empty());
        let response = module
            .execute_add_skills(
                &manager,
                &targets,
                ExecuteAddLibrarySkillsRequest {
                    request,
                    expected_token: preview.token.clone(),
                    membership: preview.membership.clone(),
                    acknowledge_redirect: false,
                },
            )
            .await
            .expect("execute add skill");
        let detail = response.library.unwrap();

        assert_eq!(response.results[0].status, LibraryAddSkillStatus::Succeeded);
        assert_eq!(detail.skills.len(), 1);
        assert_eq!(detail.skills[0].name, "api-design");
        assert_eq!(detail.skills[0].description, "Design APIs");
        assert_eq!(detail.skills[0].source, "https://example.com/repo.git");
        assert_eq!(detail.skills[0].source_type, "git");
        let saved_record = repository
            .catalogs
            .lock()
            .unwrap()
            .get(&EnvironmentKey::from_ref(&environment))
            .unwrap()
            .libraries[0]
            .skills[0]
            .clone();
        let saved: LibrarySkillSourceRecord =
            serde_json::from_value(saved_record.source_record.clone()).unwrap();
        // updatedAt 与成员记录在同一次提交中写入，并原样投影到页面摘要。
        assert!(saved_record.updated_at.is_some());
        assert_eq!(detail.skills[0].updated_at, saved_record.updated_at);
        assert_eq!(
            saved.reacquisition_url.as_deref(),
            Some("https://example.com/repo")
        );
        assert_eq!(saved.installed_revision.as_deref(), Some("tree-v1"));
        assert_eq!(saved.computed_hash.as_deref(), Some("hash-v1"));
        assert_eq!(
            saved_record.content_manifest_hash,
            detail.skills[0].content_hash
        );
        assert_eq!(
            module
                .workspace(environment.clone())
                .await
                .unwrap()
                .libraries[0]
                .skill_count,
            1
        );
        assert!(repository.payloads.lock().unwrap().contains_key(&(
            EnvironmentKey::Native,
            library_id.as_str().to_string(),
            "api-design".to_string(),
        )));
    }
}
