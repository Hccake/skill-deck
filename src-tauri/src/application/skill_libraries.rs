use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::application::collection_records::{
    CollectionRecordReader, DocumentRevision, LibraryCatalogRecordReader, SourceRecordRevision,
};
#[cfg(test)]
use crate::application::installed_skill_resolver::InstalledSkillResolver;
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
use crate::environment::types::{EnvironmentRef, SkillLocationRef};
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
/// 生效与锁定是两件事：`Confirmed` 表示配置已经起作用，`PendingAdjustment` 表示只有
/// 未完成的应用操作引用它、尚未确认生效。两者的并集才是成员锁定的判定依据。
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ExecuteAddLibrarySkillsRequest {
    pub request: PreviewAddLibrarySkillsRequest,
    pub expected_token: LibraryAddPreviewToken,
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
    pub library: SkillLibraryDetail,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct LibraryWorkspaceSnapshot {
    pub environment: EnvironmentRef,
    pub libraries: Vec<SkillLibrarySummary>,
    /// catalog 内容的摘要。应用关系不参与该摘要，页面重新进入时自行拉取最新投影。
    pub revision: String,
    pub usage_projection: Vec<LibraryUsageProjection>,
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
    Delete,
}

#[derive(Debug, Clone)]
pub(crate) struct CommitLibraryMemberRequest {
    pub(crate) environment: EnvironmentRef,
    pub(crate) library_id: LibraryId,
    pub(crate) skill_name: String,
    pub(crate) expected: LibraryMemberCommitExpectation,
    pub(crate) mutation: LibraryMemberMutation,
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
    /// 成员锁定依赖这个并集语义：任何卷入未完成操作的库都必须锁住。展示层需要区分状态时
    /// 读取每一项的 `state`，不要改变本方法的收集范围。
    fn usages<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
        library_id: &'a LibraryId,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsage>, AppError>>;

    /// 一次遍历当前 Environment 的全部 Skill 位置，聚合每个库的使用计数。
    fn usage_projection<'a>(
        &'a self,
        environment: &'a EnvironmentRef,
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsageProjection>, AppError>>;

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
    ) -> LibraryFuture<'a, Result<Vec<LibraryUsageProjection>, AppError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

pub struct SkillLibraryModule {
    repository: Arc<dyn SkillLibraryRepository>,
    usages: Arc<dyn LibraryUsageProvider>,
}

impl SkillLibraryModule {
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
        let usage_projection = self.usages.usage_projection(&environment).await?;
        workspace_snapshot(environment, catalog, usage_projection)
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

