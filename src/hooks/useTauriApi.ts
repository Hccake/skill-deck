// src/hooks/useTauriApi.ts
// 使用 tauri-specta 生成的类型安全绑定
import { commands } from '@/bindings';
import { Channel } from '@tauri-apps/api/core';
import type {
  AgentId, AgentRuntimeSnapshot, AgentSelectionGroups, ListSkillsResult, SkillScope,
  SkillUpdateInfo, FetchResult, InstallMode, SkillDeckConfig,
  SkillAuditData, DuplicateCleanupResult,
  InstallRiskPolicy, InstallRiskKind,
  DefaultTargetAgents,
  InstallTargetInfo,
  AgentDeleteImpact,
  AgentDeleteResult,
  AgentOperationWarning,
  AgentSettingsSnapshot,
  CustomAgentDefinition,
  CustomAgentDraftValidation,
  AddProjectResult, ContextRef, EnvironmentDiscoverySnapshot, EnvironmentInfo, EnvironmentRef,
  InstallWizardSessionSnapshot, MutationSnapshot,
  ProjectBinding, ProjectInfo, WslSession, ActiveMutation,
  SkillIdentity,
  InstallRequest, InstallPreview, InstallResponse, PreviewToken,
  RemovePreview, RemoveRequest, RemoveResponse,
  UpdateCheckRequest, UpdateCheckResponse,
  UpdateRequest, UpdatePreview, UpdateExecutionRequest, UpdateResponse,
  ManageAgentsPreviewRequest, ManageAgentsPreview, ManageAgentsRequest, ManageAgentsResponse,
  CopyRequest, CopyPreviewOutcome, CopyExecutionRequest, CopyResponse,
  ConfigResourceKind,
  AcquireSelectedPayloadsRequest, AcquiredPayloadHandle,
  RecoveryResourceId, RecoveryResourceStatus,
  ApplicationUpdateInfo, ApplicationUpdateProgress, ApplicationUpdateResult,
  GithubCredentialClearResult, GithubCredentialSaveResult, GithubCredentialStatus,
} from '@/bindings';

export type {
  AgentId, AgentRuntimeSnapshot, ListSkillsResult, SkillScope,
  SkillUpdateInfo, FetchResult, InstallMode, SkillDeckConfig,
  SkillAuditData, DuplicateCleanupResult,
  InstallRiskPolicy, InstallRiskKind, DefaultTargetAgents,
  InstallTargetInfo, ContextRef, EnvironmentDiscoverySnapshot, EnvironmentInfo,
  EnvironmentRef, AddProjectResult, InstallWizardSessionSnapshot, MutationSnapshot,
  ProjectBinding, ProjectInfo, WslSession,
  ActiveMutation, AgentDeleteImpact, AgentDeleteResult, AgentOperationWarning,
  AgentSettingsSnapshot, CustomAgentDefinition, CustomAgentDraftValidation,
  SkillIdentity, InstallRequest, InstallPreview, InstallResponse, PreviewToken,
  RemovePreview, RemoveRequest, RemoveResponse,
  UpdateCheckRequest, UpdateCheckResponse,
  UpdateRequest, UpdatePreview, UpdateExecutionRequest, UpdateResponse,
  ManageAgentsPreviewRequest, ManageAgentsPreview, ManageAgentsRequest, ManageAgentsResponse,
  CopyRequest, CopyPreviewOutcome, CopyExecutionRequest, CopyResponse,
  ConfigResourceKind,
  AcquireSelectedPayloadsRequest, AcquiredPayloadHandle,
  RecoveryResourceId, RecoveryResourceStatus,
  ApplicationUpdateInfo, ApplicationUpdateProgress, ApplicationUpdateResult,
  GithubCredentialClearResult, GithubCredentialSaveResult, GithubCredentialStatus,
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
export async function listAgents(context: ContextRef): Promise<AgentRuntimeSnapshot> {
  return unwrap(await commands.listAgents(context));
}

export async function listAgentSelectionGroups(context: ContextRef): Promise<AgentSelectionGroups> {
  return unwrap(await commands.listAgentSelectionGroups(context));
}

export async function getAgentSettingsSnapshot(
  context: ContextRef,
): Promise<AgentSettingsSnapshot> {
  return commands.getAgentSettingsSnapshot(context);
}

export async function validateCustomAgentDraft(
  context: ContextRef,
  draft: CustomAgentDefinition,
): Promise<CustomAgentDraftValidation> {
  return unwrap(await commands.validateCustomAgentDraft(context, draft));
}

export async function saveCustomAgent(
  context: ContextRef,
  draft: CustomAgentDefinition,
  expectedRegistryRevision: string,
): Promise<AgentSettingsSnapshot> {
  return unwrap(await commands.saveCustomAgent(context, draft, expectedRegistryRevision));
}

export async function duplicateCustomAgentDraft(
  sourceId: AgentId,
  newId: AgentId,
): Promise<CustomAgentDefinition> {
  return unwrap(await commands.duplicateCustomAgentDraft(sourceId, newId));
}

export async function previewCustomAgentDelete(
  context: ContextRef,
  id: AgentId,
  expectedRegistryRevision: string,
): Promise<AgentDeleteImpact> {
  return unwrap(await commands.previewCustomAgentDelete(context, id, expectedRegistryRevision));
}

export async function deleteCustomAgent(
  context: ContextRef,
  id: AgentId,
  expectedRegistryRevision: string,
): Promise<AgentDeleteResult> {
  return unwrap(await commands.deleteCustomAgent(context, id, expectedRegistryRevision));
}

export async function deleteInvalidCustomAgent(
  context: ContextRef,
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

/**
 * 列出 Eve project 内可安装的具体目标（root agent 与 subagents）。
 */
export async function listEveInstallTargets(context: ContextRef): Promise<InstallTargetInfo[]> {
  return unwrap(await commands.listEveInstallTargets(context));
}

/**
 * 列出已安装的 Skills
 */
export async function listSkills(context: ContextRef): Promise<ListSkillsResult> {
  return unwrap(await commands.listSkills(context));
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

// ============ Agent 选择相关 API ============

/**
 * 获取 GUI scope-aware 默认安装目标
 */
export async function getDefaultTargetAgents(
  context: ContextRef,
): Promise<DefaultTargetAgents | null> {
  return unwrap(await commands.getDefaultTargetAgents(context));
}

/**
 * 保存 GUI scope-aware 默认安装目标
 */
export async function saveDefaultTargetAgents(
  context: ContextRef,
  defaults: DefaultTargetAgents,
  expectedRegistryRevision: string,
): Promise<void> {
  unwrap(await commands.saveDefaultTargetAgents(
    context,
    defaults,
    expectedRegistryRevision,
  ));
}

// ============ 安装相关 API ============

/**
 * 从来源获取可用的 skills 列表
 */
export async function fetchAvailable(
  context: ContextRef,
  source: string,
  operationId: string,
): Promise<FetchResult> {
  return unwrap(await commands.fetchAvailable(context, source, operationId));
}

export async function acquireSelectedPayloads(
  request: AcquireSelectedPayloadsRequest,
): Promise<AcquiredPayloadHandle[]> {
  return unwrap(await commands.acquireSelectedPayloads(request));
}

export async function previewInstall(request: InstallRequest): Promise<InstallPreview> {
  return unwrap(await commands.previewInstall(request));
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
  context: ContextRef,
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
  context: ContextRef,
  kind: ConfigResourceKind,
): Promise<void> {
  unwrap(await commands.openConfigResource(context, kind));
}

// ============ Environment / mutation API ============

export async function listEnvironments(): Promise<EnvironmentDiscoverySnapshot> {
  return unwrap(await commands.listEnvironments());
}

export async function connectEnvironment(distroName: string): Promise<WslSession> {
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

export async function retryHostProjectMigration(): Promise<ProjectInfo[]> {
  return unwrap(await commands.retryHostProjectMigration());
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
): Promise<UpdateResponse> {
  return unwrap(await commands.updateSkill(execution, expectedToken));
}

/**
 * 批量更新多个 skills（同源 clone 合并）
 */
export async function updateSkillsBatch(
  execution: UpdateExecutionRequest,
  expectedToken: PreviewToken,
): Promise<UpdateResponse> {
  return unwrap(await commands.updateSkillsBatch(execution, expectedToken));
}

// ============ 安全审计 API ============

/**
 * 检查 skill 安全审计数据
 * 3 秒超时，graceful degradation
 */
export async function checkSkillAudit(
  source: string,
  skills: string[]
): Promise<Partial<Record<string, SkillAuditData>> | null> {
  return unwrap(await commands.checkSkillAudit(source, skills));
}

// ============ 向导窗口 API ============

/**
 * 打开安装向导独立窗口
 */
export async function openInstallWizard(params: {
  entryPoint: string;
  context: ContextRef;
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
): Promise<ManageAgentsPreview> {
  return unwrap(await commands.previewManageSkillAgents(request));
}

export async function manageSkillAgents(
  request: ManageAgentsRequest,
): Promise<ManageAgentsResponse> {
  return unwrap(await commands.manageSkillAgents(request));
}

export async function cleanupDuplicateAgentCopies(
  context: ContextRef,
  params: { skillName: string; agents: AgentId[] },
): Promise<DuplicateCleanupResult[]> {
  return unwrap(await commands.cleanupDuplicateAgentCopies(
    context,
    params.skillName,
    params.agents,
  ));
}

// ============ 复制 Skill API ============

/**
 * 复制项目级 skill 到其他项目
 */
export async function previewCopySkillToProjects(request: CopyRequest): Promise<CopyPreviewOutcome> {
  return unwrap(await commands.previewCopySkillToProjects(request));
}

export async function copySkillToProjects(request: CopyExecutionRequest): Promise<CopyResponse> {
  return unwrap(await commands.copySkillToProjects(request));
}
