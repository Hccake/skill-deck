// src/hooks/useTauriApi.ts
// 使用 tauri-specta 生成的类型安全绑定
import { commands } from '@/bindings';
import type {
  AgentInfo, AgentType, ListSkillsResult, SkillScope, RemoveResult,
  SkillUpdateInfo, UpdateSkillResponse, FetchResult, InstallMode,
  InstallParams, InstallResults, SkillDeckConfig,
  SkillAuditData, SkillAgentDetails, ManageAgentsResult, DuplicateCleanupResult,
  CopySkillResult, CopyProjectResult,
  InstallRiskPolicy, InstallRiskKind,
  DefaultTargetAgents,
  InstallTargetInfo,
  InstallTargetSpec,
  AddProjectResult, ContextRef, EnvironmentDiscoverySnapshot, EnvironmentInfo, EnvironmentRef,
  MutationSnapshot,
  ProjectBinding, ProjectInfo, WslSession, ActiveMutation,
} from '@/bindings';

export type {
  AgentInfo, AgentType, ListSkillsResult, SkillScope, RemoveResult,
  SkillUpdateInfo, UpdateSkillResponse, FetchResult, InstallMode,
  InstallParams, InstallResults, SkillDeckConfig,
  SkillAuditData, SkillAgentDetails, ManageAgentsResult, DuplicateCleanupResult,
  CopySkillResult, CopyProjectResult,
  InstallRiskPolicy, InstallRiskKind, DefaultTargetAgents,
  InstallTargetInfo, InstallTargetSpec, ContextRef, EnvironmentDiscoverySnapshot, EnvironmentInfo,
  EnvironmentRef, AddProjectResult, MutationSnapshot, ProjectBinding, ProjectInfo, WslSession,
  ActiveMutation,
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
export async function listAgents(context: ContextRef): Promise<AgentInfo[]> {
  return unwrap(await commands.listAgents(context));
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
  context: ContextRef,
  canonicalPath: string,
): Promise<string> {
  return unwrap(await commands.readSkillContent(context, canonicalPath));
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
): Promise<void> {
  unwrap(await commands.saveDefaultTargetAgents(context, defaults));
}

// ============ 安装相关 API ============

/**
 * 从来源获取可用的 skills 列表
 */
export async function fetchAvailable(
  context: ContextRef,
  source: string,
): Promise<FetchResult> {
  return unwrap(await commands.fetchAvailable(context, source));
}

/**
 * 安装选中的 skills
 */
export async function installSkills(
  context: ContextRef,
  params: InstallParams,
): Promise<InstallResults> {
  return unwrap(await commands.installSkills(context, params));
}

/**
 * 检测覆盖情况
 */
export async function checkOverwrites(
  context: ContextRef,
  skills: string[],
  agents: string[],
  privateCopyAgents: string[] = [],
  agentTargets: InstallTargetSpec[] = [],
): Promise<Partial<Record<string, string[]>>> {
  return unwrap(await commands.checkOverwrites(
    context,
    skills,
    agents,
    privateCopyAgents,
    agentTargets,
  ));
}

// ============ 删除相关 API ============

/**
 * 删除指定 skill
 * @param params.fullRemoval - true=完全删除，false=部分移除（仅删除指定 agents 的 symlink）
 * @param params.agents - 部分移除时指定的 agent 列表
 */
export async function removeSkill(
  context: ContextRef,
  params: {
    name: string;
    agents?: AgentType[];
    fullRemoval?: boolean;
    agentTargets?: InstallTargetSpec[];
  },
): Promise<RemoveResult> {
  return unwrap(await commands.removeSkill(
    context,
    params.name,
    params.agents ?? null,
    params.fullRemoval ?? null,
    params.agentTargets ?? null,
  ));
}

/**
 * 查询 skill 的 agent 安装详情（智能删除对话框用）
 */
export async function getSkillAgentDetails(
  context: ContextRef,
  name: string,
): Promise<SkillAgentDetails> {
  return unwrap(await commands.getSkillAgentDetails(context, name));
}

/**
 * 在系统文件管理器中打开路径
 */
export async function openInExplorer(path: string): Promise<void> {
  unwrap(await commands.openInExplorer(path));
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
export async function checkUpdates(context: ContextRef): Promise<SkillUpdateInfo[]> {
  return unwrap(await commands.checkUpdates(context));
}

/**
 * 更新指定 skill
 */
export async function updateSkill(
  context: ContextRef,
  name: string,
): Promise<UpdateSkillResponse> {
  return unwrap(await commands.updateSkill(context, name));
}

/**
 * 批量更新多个 skills（同源 clone 合并）
 */
export async function updateSkillsBatch(
  context: ContextRef,
  names: string[],
): Promise<UpdateSkillResponse> {
  return unwrap(await commands.updateSkillsBatch(context, names));
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

// ============ Agent 管理 API ============

/**
 * 管理 skill 的 agent 支持（添加/移除）
 */
export async function manageSkillAgents(
  context: ContextRef,
  params: {
    skillName: string;
    addAgents: AgentType[];
    removeAgents: AgentType[];
    privateCopyAgents?: AgentType[];
    mode: InstallMode;
  },
): Promise<ManageAgentsResult> {
  return unwrap(await commands.manageSkillAgents(
    context,
    params.skillName,
    params.addAgents,
    params.removeAgents,
    params.privateCopyAgents ?? [],
    params.mode,
  ));
}

export async function cleanupDuplicateAgentCopies(
  context: ContextRef,
  params: { skillName: string; agents: AgentType[] },
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
export async function copySkillToProjects(params: {
  skillName: string;
  source: ContextRef;
  targets: ContextRef[];
  agents: string[];
  privateCopyAgents?: string[];
}): Promise<CopySkillResult> {
  return unwrap(await commands.copySkillToProjects(
    params.skillName,
    params.source,
    params.targets,
    params.agents,
    params.privateCopyAgents ?? [],
  ));
}
