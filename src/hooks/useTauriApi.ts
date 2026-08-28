// src/hooks/useTauriApi.ts
// 使用 tauri-specta 生成的类型安全绑定
import { commands } from '@/bindings';
import { Channel } from '@tauri-apps/api/core';
import type {
  AgentId, AgentRuntimeSnapshot, ListSkillsResult, InstalledSkillLocation,
  SkillUpdateInfo, FetchResult, InstallMode, SkillDeckConfig,
  AgentDeleteImpact,
  AgentDeleteResult,
  AgentSettingsSnapshot,
  CustomAgentDefinition,
  CustomAgentDraftValidation,
  AddProjectResult, SkillLocationRef, EnvironmentDiscoverySnapshot, EnvironmentInfo, EnvironmentRef,
  InstallWizardSessionSnapshot, MutationSnapshot,
  RegisteredProject, ProjectInfo, ActiveMutation,
  SkillIdentity,
  AgentSelectionIntent, AgentSelectionSubmission, ConfirmInstallAgentSelectionOutcome,
  InstallRequest, InstallPreview, InstallPreviewOutcome, InstallResponse,
  InstallAgentSelectionSnapshot, PreviewToken,
  RemovePreview, RemoveRequest, RemoveResponse,
  UpdateCheckRequest, UpdateCheckResponse,
  UpdateRequest, UpdatePreview, UpdateExecutionRequest, UpdateResponse,
  ManageAgentSelectionSnapshot, ManageAgentsPreviewRequest, ManageAgentsPreview,
  ManageAgentsPreviewOutcome, ManageAgentsRequest, ManageAgentsResponse,
  CopyAgentSelectionSnapshot, CopyRequest, CopyPreviewOutcome, CopyExecutionRequest, CopyResponse,
  ConfigResourceKind,
  AcquireSelectedPayloadsRequest, AcquiredPayloadHandle,
  RecoveryResourceId, RecoveryResourceStatus,
  ApplicationUpdateInfo, ApplicationUpdateProgress, ApplicationUpdateResult,
  GithubCredentialClearResult, GithubCredentialSaveResult, GithubCredentialStatus,
  NetworkProxySettings, ProxyConnectionTestResult,
  DiscoverSearchPayload, DiscoverLeaderboardPayload, DiscoverLeaderboardTab,
  SourceSelectionIntent,
  ExecuteAddLibrarySkillsRequest, LibraryAddPreview, LibraryAddResponse,
  PreviewAddLibrarySkillsRequest, LibraryId, LibraryWorkspaceSnapshot, SkillLibraryDetail,
  ApplyLibraryApplicationRequest, LibraryApplicationDraft, LibraryApplicationPreview,
  LibraryApplicationResponse, LibraryApplicationSummary,
  LibraryAgentOptions,
  ExecuteLibraryUpdateRequest, LibraryUpdateExecutionOutcome, LibraryUpdatePreview,
  LibraryUpdateContinuation, LibraryUpdatePreviewToken, LibraryUpdateRiskConfirmation,
  UpdateLibrarySkillsRequest,
  RemoveLibrarySkillRequest,
  LibraryUsage,
} from '@/bindings';

export type {
  AgentId, AgentRuntimeSnapshot, ListSkillsResult, InstalledSkillLocation,
  SkillUpdateInfo, FetchResult, InstallMode, SkillDeckConfig,
  SkillLocationRef, EnvironmentDiscoverySnapshot, EnvironmentInfo,
  EnvironmentRef, AddProjectResult, InstallWizardSessionSnapshot, MutationSnapshot,
  RegisteredProject, ProjectInfo,
  ActiveMutation, AgentDeleteImpact, AgentDeleteResult,
  AgentSettingsSnapshot, CustomAgentDefinition, CustomAgentDraftValidation,
  SkillIdentity, AgentSelectionSubmission, ConfirmInstallAgentSelectionOutcome,
  InstallRequest, InstallPreview, InstallPreviewOutcome, InstallResponse,
  InstallAgentSelectionSnapshot, PreviewToken,
  RemovePreview, RemoveRequest, RemoveResponse,
  UpdateCheckRequest, UpdateCheckResponse,
  UpdateRequest, UpdatePreview, UpdateExecutionRequest, UpdateResponse,
  ManageAgentSelectionSnapshot, ManageAgentsPreviewRequest, ManageAgentsPreview,
  ManageAgentsPreviewOutcome, ManageAgentsRequest, ManageAgentsResponse,
  CopyAgentSelectionSnapshot, CopyRequest, CopyPreviewOutcome, CopyExecutionRequest, CopyResponse,
  ConfigResourceKind,
  AcquireSelectedPayloadsRequest, AcquiredPayloadHandle,
  RecoveryResourceId, RecoveryResourceStatus,
  ApplicationUpdateInfo, ApplicationUpdateProgress, ApplicationUpdateResult,
  GithubCredentialClearResult, GithubCredentialSaveResult, GithubCredentialStatus,
  NetworkProxySettings, ProxyConnectionTestResult,
  DiscoverSearchPayload, DiscoverLeaderboardPayload, DiscoverLeaderboardTab,
  ExecuteAddLibrarySkillsRequest, LibraryAddPreview, LibraryAddResponse,
  PreviewAddLibrarySkillsRequest, LibraryId, LibraryWorkspaceSnapshot, SkillLibraryDetail,
  ApplyLibraryApplicationRequest, LibraryApplicationDraft, LibraryApplicationPreview,
  LibraryApplicationResponse, LibraryApplicationSummary,
  LibraryAgentOptions,
  ExecuteLibraryUpdateRequest, LibraryUpdateExecutionOutcome, LibraryUpdatePreview,
  LibraryUpdateContinuation, LibraryUpdatePreviewToken, LibraryUpdateRiskConfirmation,
  UpdateLibrarySkillsRequest,
  RemoveLibrarySkillRequest,
  LibraryUsage,
};

/** 解包 tauri-specta Result 类型，error 时抛出异常（保持与原有 invoke 行为一致） */
function unwrap<T, E>(result: { status: "ok"; data: T } | { status: "error"; error: E }): T {
  if (result.status === "ok") return result.data;
  throw result.error;
}

/**
 * 列出所有 Agents（包括未安装的）
 * 返回完整信息供前端使用，前端无需额外计算
 */
export async function listAgents(context: SkillLocationRef): Promise<AgentRuntimeSnapshot> {
  return unwrap(await commands.listAgents(context));
}

export async function getAgentSettingsSnapshot(
  context: SkillLocationRef,
): Promise<AgentSettingsSnapshot> {
  return commands.getAgentSettingsSnapshot(context);
}

export async function getAgentLibraryUsages(
  environment: EnvironmentRef,
  id: AgentId,
): Promise<LibraryUsage[]> {
  return unwrap(await commands.getAgentLibraryUsages(environment, id));
}

export async function validateCustomAgentDraft(
  context: SkillLocationRef,
  draft: CustomAgentDefinition,
): Promise<CustomAgentDraftValidation> {
  return unwrap(await commands.validateCustomAgentDraft(context, draft));
}

export async function saveCustomAgent(
  context: SkillLocationRef,
  draft: CustomAgentDefinition,
  originalId: AgentId | null,
  expectedRegistryRevision: string,
): Promise<AgentSettingsSnapshot> {
  return unwrap(await commands.saveCustomAgent(
    context,
    draft,
    originalId,
    expectedRegistryRevision,
  ));
}

export async function previewCustomAgentDelete(
  context: SkillLocationRef,
  id: AgentId,
  expectedRegistryRevision: string,
): Promise<AgentDeleteImpact> {
  return unwrap(await commands.previewCustomAgentDelete(context, id, expectedRegistryRevision));
}

export async function deleteCustomAgent(
  context: SkillLocationRef,
  id: AgentId,
  expectedRegistryRevision: string,
): Promise<AgentDeleteResult> {
  return unwrap(await commands.deleteCustomAgent(context, id, expectedRegistryRevision));
}

export async function deleteInvalidCustomAgent(
  context: SkillLocationRef,
  index: number,
  expectedRegistryRevision: string,
): Promise<AgentDeleteResult> {
  return unwrap(await commands.deleteInvalidCustomAgent(context, index, expectedRegistryRevision));
}

export async function getRecoveryResourceStatus(
  resourceId: RecoveryResourceId,
): Promise<RecoveryResourceStatus> {
  return unwrap(await commands.getRecoveryResourceStatus(resourceId));
}

export async function listRecoveryResources(): Promise<RecoveryResourceStatus[]> {
  return unwrap(await commands.listRecoveryResources());
}

export async function confirmRecoveryResourceResolved(
  resourceId: RecoveryResourceId,
  expectedRevision: string,
): Promise<void> {
  unwrap(await commands.confirmRecoveryResourceResolved(resourceId, expectedRevision));
}

export async function openRecoveryResource(resourceId: RecoveryResourceId): Promise<void> {
  unwrap(await commands.openRecoveryResource(resourceId));
}

export async function checkApplicationUpdate(): Promise<ApplicationUpdateInfo | null> {
  return unwrap(await commands.checkApplicationUpdate());
}

export async function downloadAndInstallApplicationUpdate(
  expectedVersion: string,
  onProgress: (event: ApplicationUpdateProgress) => void,
): Promise<ApplicationUpdateResult> {
  const progress = new Channel<ApplicationUpdateProgress>();
  progress.onmessage = onProgress;
  return unwrap(await commands.downloadAndInstallApplicationUpdate(expectedVersion, progress));
}

export async function cancelApplicationUpdateDownload(): Promise<boolean> {
  return unwrap(await commands.cancelApplicationUpdateDownload());
}

/**
 * 列出已安装的 Skills
 */
export async function listSkills(context: SkillLocationRef): Promise<ListSkillsResult> {
  return unwrap(await commands.listSkills(context));
}

export async function listSkillLibraries(
  environment: EnvironmentRef,
): Promise<LibraryWorkspaceSnapshot> {
  return unwrap(await commands.listSkillLibraries(environment));
}

export async function createSkillLibrary(
  environment: EnvironmentRef,
  name: string,
): Promise<LibraryWorkspaceSnapshot> {
  return unwrap(await commands.createSkillLibrary(environment, name));
}

export async function renameSkillLibrary(
  environment: EnvironmentRef,
  libraryId: LibraryId,
  name: string,
): Promise<LibraryWorkspaceSnapshot> {
  return unwrap(await commands.renameSkillLibrary(environment, libraryId, name));
}

export async function getSkillLibrary(
  environment: EnvironmentRef,
  libraryId: LibraryId,
): Promise<SkillLibraryDetail> {
  return unwrap(await commands.getSkillLibrary(environment, libraryId));
}

export async function readLibrarySkillContent(
  environment: EnvironmentRef,
  libraryId: LibraryId,
  skillName: string,
): Promise<string> {
  return unwrap(await commands.readLibrarySkillContent(environment, libraryId, skillName));
}

export async function discoverSkillSource(
  environment: EnvironmentRef,
  source: string,
  operationId: string,
  selectionIntent: SourceSelectionIntent = {
    wildcardRequested: false,
    explicitSkillNames: [],
  },
): Promise<FetchResult> {
  return unwrap(await commands.discoverSkillSource(
    environment,
    source,
    operationId,
    selectionIntent,
  ));
}

export async function addSkillsToLibrary(
  request: ExecuteAddLibrarySkillsRequest,
): Promise<LibraryAddResponse> {
  return unwrap(await commands.addSkillsToLibrary(request));
}

export async function previewAddLibrarySkills(
  request: PreviewAddLibrarySkillsRequest,
): Promise<LibraryAddPreview> {
  return unwrap(await commands.previewAddLibrarySkills(request));
}

export async function getLibraryApplication(
  context: SkillLocationRef,
): Promise<LibraryApplicationSummary> {
  return unwrap(await commands.getLibraryApplication(context));
}

export async function getLibraryAgentOptions(
  context: SkillLocationRef,
): Promise<LibraryAgentOptions> {
  return unwrap(await commands.getLibraryAgentOptions(context));
}

export async function previewLibraryApplication(
  draft: LibraryApplicationDraft,
): Promise<LibraryApplicationPreview> {
  return unwrap(await commands.previewLibraryApplication(draft));
}

export async function applyLibraryApplication(
  request: ApplyLibraryApplicationRequest,
): Promise<LibraryApplicationResponse> {
  return unwrap(await commands.applyLibraryApplication(request));
}

export async function retryLibraryApplication(
  context: SkillLocationRef,
): Promise<LibraryApplicationResponse> {
  return unwrap(await commands.retryLibraryApplication(context));
}

export async function checkLibrarySkillUpdates(
  environment: EnvironmentRef,
  libraryId: LibraryId,
): Promise<UpdateCheckResponse> {
  return unwrap(await commands.checkLibrarySkillUpdates(environment, libraryId));
}

export async function updateLibrarySkills(
  request: ExecuteLibraryUpdateRequest,
): Promise<LibraryUpdateExecutionOutcome> {
  return unwrap(await commands.updateLibrarySkills(request));
}

export async function previewLibrarySkillUpdates(
  request: UpdateLibrarySkillsRequest,
): Promise<LibraryUpdatePreview> {
  return unwrap(await commands.previewLibrarySkillUpdates(request));
}

export async function removeLibrarySkill(
  request: RemoveLibrarySkillRequest,
): Promise<SkillLibraryDetail> {
  return unwrap(await commands.removeLibrarySkill(request));
}

export async function deleteSkillLibrary(
  environment: EnvironmentRef,
  libraryId: LibraryId,
): Promise<LibraryWorkspaceSnapshot> {
  return unwrap(await commands.deleteSkillLibrary(environment, libraryId));
}

/**
 * Read SKILL.md content (markdown body, frontmatter stripped).
 * Takes the skill's canonical directory path.
 */
export async function readSkillContent(
  identity: SkillIdentity,
): Promise<string> {
  return unwrap(await commands.readSkillContent(identity));
}

// ============ 配置相关 API ============

/**
 * 获取应用配置
 */
export async function getConfig(): Promise<SkillDeckConfig> {
  return unwrap(await commands.getConfig());
}

/**
 * 保存应用配置
 */
export async function saveConfig(config: SkillDeckConfig): Promise<void> {
  unwrap(await commands.saveConfig(config));
}

export async function getProxySettings(): Promise<NetworkProxySettings> {
  return unwrap(await commands.getProxySettings());
}

export async function saveProxySettings(
  settings: NetworkProxySettings,
): Promise<NetworkProxySettings> {
  return unwrap(await commands.saveProxySettings(settings));
}

export async function testProxyConnection(
  settings: NetworkProxySettings,
  wslDistros: string[],
): Promise<ProxyConnectionTestResult> {
  return unwrap(await commands.testProxyConnection(settings, wslDistros));
}

export async function searchDiscoverSkillsTransport(
  query: string,
): Promise<DiscoverSearchPayload> {
  return unwrap(await commands.searchDiscoverSkills(query));
}

export async function getDiscoverLeaderboardTransport(
  tab: DiscoverLeaderboardTab,
): Promise<DiscoverLeaderboardPayload> {
  return unwrap(await commands.getDiscoverLeaderboard(tab));
}

export async function getDiscoverSkillDetailTransport(
  source: string,
  skill: string,
): Promise<string> {
  return unwrap(await commands.getDiscoverSkillDetail(source, skill));
}

export async function setWslIntegrationEnabled(
  enabled: boolean,
): Promise<EnvironmentDiscoverySnapshot> {
  return unwrap(await commands.setWslIntegrationEnabled(enabled));
}

export async function getGithubCredentialStatus(): Promise<GithubCredentialStatus> {
  return unwrap(await commands.getGithubCredentialStatus());
}

export async function saveGithubCredential(
  token: string,
): Promise<GithubCredentialSaveResult> {
  return unwrap(await commands.saveGithubCredential(token));
}

export async function clearGithubCredential(): Promise<GithubCredentialClearResult> {
  return unwrap(await commands.clearGithubCredential());
}

// ============ 安装相关 API ============

export async function acquireSelectedPayloads(
  request: AcquireSelectedPayloadsRequest,
): Promise<AcquiredPayloadHandle[]> {
  return unwrap(await commands.acquireSelectedPayloads(request));
}

export async function previewInstall(request: InstallRequest): Promise<InstallPreviewOutcome> {
  return unwrap(await commands.previewInstall(request));
}

export async function getInstallAgentSelection(
  context: SkillLocationRef,
  agentSelectionIntent: AgentSelectionIntent,
): Promise<InstallAgentSelectionSnapshot> {
  return unwrap(await commands.getInstallAgentSelection(context, agentSelectionIntent));
}

export async function confirmInstallAgentSelection(
  context: SkillLocationRef,
  submission: AgentSelectionSubmission,
  agentSelectionIntent: AgentSelectionIntent,
): Promise<ConfirmInstallAgentSelectionOutcome> {
  return unwrap(await commands.confirmInstallAgentSelection(
    context,
    submission,
    agentSelectionIntent,
  ));
}

/**
 * 安装选中的 skills
 */
export async function installSkills(
  request: InstallRequest,
  expectedToken: PreviewToken,
): Promise<InstallResponse> {
  return unwrap(await commands.installSkills(request, expectedToken));
}

// ============ 删除相关 API ============

/**
 * 删除指定 skill
 * @param params.fullRemoval - true=完全删除，false=部分移除（仅删除指定 agents 的 symlink）
 * @param params.agents - 部分移除时指定的 agent 列表
 */
export async function previewRemove(
  context: SkillLocationRef,
  skillName: string,
): Promise<RemovePreview> {
  return unwrap(await commands.previewRemove(context, skillName));
}

export async function removeSkill(request: RemoveRequest): Promise<RemoveResponse> {
  return unwrap(await commands.removeSkill(request));
}

export async function openSkillResource(identity: SkillIdentity): Promise<void> {
  unwrap(await commands.openSkillResource(identity));
}

export async function openConfigResource(
  context: SkillLocationRef,
  kind: ConfigResourceKind,
): Promise<void> {
  unwrap(await commands.openConfigResource(context, kind));
}

// ============ Environment / mutation API ============

export async function listEnvironments(): Promise<EnvironmentDiscoverySnapshot> {
  return unwrap(await commands.listEnvironments());
}

export async function connectEnvironment(distroName: string): Promise<EnvironmentInfo> {
  return unwrap(await commands.connectEnvironment(distroName));
}

export async function mapEnvironmentPath(
  environment: EnvironmentRef,
  path: string,
): Promise<string> {
  return unwrap(await commands.mapEnvironmentPath(environment, path));
}

export async function listEnvironmentProjects(
  environment: EnvironmentRef,
): Promise<ProjectInfo[]> {
  return unwrap(await commands.listEnvironmentProjects(environment));
}

export async function addEnvironmentProject(
  environment: EnvironmentRef,
  nativePath: string,
): Promise<AddProjectResult> {
  return unwrap(await commands.addEnvironmentProject(environment, nativePath));
}

export async function removeEnvironmentProject(
  environment: EnvironmentRef,
  projectId: string,
): Promise<ProjectInfo[]> {
  return unwrap(await commands.removeEnvironmentProject(environment, projectId));
}

export async function setEnvironmentProjectCrossStorageWarning(
  environment: EnvironmentRef,
  projectId: string,
  suppressed: boolean,
): Promise<ProjectInfo> {
  return unwrap(await commands.setEnvironmentProjectCrossStorageWarning(
    environment,
    projectId,
    suppressed,
  ));
}

export async function retryNativeProjectMigration(): Promise<ProjectInfo[]> {
  return unwrap(await commands.retryNativeProjectMigration());
}

export async function getActiveMutation(): Promise<MutationSnapshot> {
  return await commands.getActiveMutation();
}

export async function requestCancelActiveMutation(): Promise<boolean> {
  return unwrap(await commands.requestCancelActiveMutation());
}

// ============ 更新检测 API ============

/**
 * 检测指定 scope 的 skills 是否有更新
 */
export async function checkUpdates(request: UpdateCheckRequest): Promise<UpdateCheckResponse> {
  return unwrap(await commands.checkUpdates(request));
}

export async function previewUpdate(request: UpdateRequest): Promise<UpdatePreview> {
  return unwrap(await commands.previewUpdate(request));
}

/**
 * 更新指定 skill
 */
export async function updateSkill(
  execution: UpdateExecutionRequest,
  expectedToken: PreviewToken,
  acknowledgeRedirect = false,
): Promise<UpdateResponse> {
  return unwrap(await commands.updateSkill(execution, expectedToken, acknowledgeRedirect));
}

/**
 * 批量更新多个 skills（同源 clone 合并）
 */
export async function updateSkillsBatch(
  execution: UpdateExecutionRequest,
  expectedToken: PreviewToken,
  acknowledgeRedirect = false,
): Promise<UpdateResponse> {
  return unwrap(await commands.updateSkillsBatch(execution, expectedToken, acknowledgeRedirect));
}

// ============ 向导窗口 API ============

/**
 * 打开安装向导独立窗口
 */
export async function openInstallWizard(params: {
  entryPoint: string;
  context: SkillLocationRef;
  projectPath?: string;
  prefillSource?: string;
  prefillSkillName?: string;
}): Promise<void> {
  unwrap(
    await commands.openInstallWizard(
      params.entryPoint,
      params.context,
      params.projectPath ?? null,
      params.prefillSource ?? null,
      params.prefillSkillName ?? null,
    )
  );
}

export async function getInstallWizardSession(): Promise<InstallWizardSessionSnapshot> {
  return commands.getInstallWizardSession();
}

export async function focusInstallWizard(): Promise<boolean> {
  return unwrap(await commands.focusInstallWizard());
}

// ============ Agent 管理 API ============

/**
 * 管理 skill 的 agent 支持（添加/移除）
 */
export async function previewManageSkillAgents(
  request: ManageAgentsPreviewRequest,
): Promise<ManageAgentsPreviewOutcome> {
  return unwrap(await commands.previewManageSkillAgents(request));
}

export async function getManageAgentSelection(
  context: SkillLocationRef,
  skillName: string,
): Promise<ManageAgentSelectionSnapshot> {
  return unwrap(await commands.getManageAgentSelection(context, skillName));
}

export async function manageSkillAgents(
  request: ManageAgentsRequest,
): Promise<ManageAgentsResponse> {
  return unwrap(await commands.manageSkillAgents(request));
}

// ============ 复制 Skill API ============

export async function getCopyAgentSelection(
  source: SkillLocationRef,
  skillName: string,
): Promise<CopyAgentSelectionSnapshot> {
  return unwrap(await commands.getCopyAgentSelection(source, skillName));
}

/**
 * 复制项目级 skill 到其他项目
 */
export async function previewCopySkillToProjects(request: CopyRequest): Promise<CopyPreviewOutcome> {
  return unwrap(await commands.previewCopySkillToProjects(request));
}

export async function copySkillToProjects(request: CopyExecutionRequest): Promise<CopyResponse> {
  return unwrap(await commands.copySkillToProjects(request));
}
