// src/hooks/useTauriApi.ts
// 使用 tauri-specta 生成的类型安全绑定
import { commands } from '@/bindings';
import type {
  AgentInfo, AgentType, ListSkillsResult, SkillScope, RemoveResult,
  SkillUpdateInfo, UpdateSkillResponse, FetchResult, InstallMode,
  InstallParams, InstallResults, SkillDeckConfig, Scope,
  SkillAuditData, SkillAgentDetails, ManageAgentsResult, DuplicateCleanupResult,
  CopySkillResult, CopyProjectResult, ProjectSkillStatus,
  InstallRiskPolicy, InstallRiskKind,
  DefaultTargetAgents,
  InstallTargetInfo,
  InstallTargetSpec,
  ContextRef, EnvironmentInfo, EnvironmentRef, ProjectBinding, WslSession, ActiveMutation,
} from '@/bindings';

export type {
  AgentInfo, AgentType, ListSkillsResult, SkillScope, RemoveResult,
  SkillUpdateInfo, UpdateSkillResponse, FetchResult, InstallMode,
  InstallParams, InstallResults, SkillDeckConfig,
  SkillAuditData, SkillAgentDetails, ManageAgentsResult, DuplicateCleanupResult,
  CopySkillResult, CopyProjectResult, ProjectSkillStatus,
  InstallRiskPolicy, InstallRiskKind, DefaultTargetAgents,
  InstallTargetInfo, InstallTargetSpec, ContextRef, EnvironmentInfo, EnvironmentRef,
  ProjectBinding, WslSession, ActiveMutation,
};

/** 解包 tauri-specta Result 类型，error 时抛出异常（保持与原有 invoke 行为一致） */
function unwrap<T, E>(result: { status: "ok"; data: T } | { status: "error"; error: E }): T {
  if (result.status === "ok") return result.data;
  throw result.error;
}

/** list_skills 参数 */
interface ListSkillsParams {
  scope?: SkillScope;
  projectPath?: string;
}

/**
 * 列出所有 Agents（包括未安装的）
 * 返回完整信息供前端使用，前端无需额外计算
 */
export async function listAgents(): Promise<AgentInfo[]> {
  return unwrap(await commands.listAgents());
}

/**
 * 按指定项目路径列出 Agents，project-only Agent 会基于该路径检测。
 */
export async function listAgentsForProject(projectPath?: string): Promise<AgentInfo[]> {
  return unwrap(await commands.listAgentsForProject(projectPath ?? null));
}

export async function listAgentsForProjectV2(context: ContextRef): Promise<AgentInfo[]> {
  return unwrap(await commands.listAgentsForProjectV2(context));
}

/**
 * 列出 Eve project 内可安装的具体目标（root agent 与 subagents）。
 */
export async function listEveInstallTargets(projectPath: string): Promise<InstallTargetInfo[]> {
  return unwrap(await commands.listEveInstallTargets(projectPath));
}

/**
 * 列出已安装的 Skills
 */
export async function listSkills(params?: ListSkillsParams): Promise<ListSkillsResult> {
  return unwrap(await commands.listSkills({
    scope: params?.scope ?? null,
    projectPath: params?.projectPath ?? null,
  }));
}

export async function listSkillsV2(context: ContextRef): Promise<ListSkillsResult> {
  return unwrap(await commands.listSkillsV2(context));
}

/**
 * Read SKILL.md content (markdown body, frontmatter stripped).
 * Takes the skill's canonical directory path.
 */
export async function readSkillContent(canonicalPath: string): Promise<string> {
  return unwrap(await commands.readSkillContent(canonicalPath));
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
 * 获取上次选择的 agents
 */
export async function getLastSelectedAgents(): Promise<string[]> {
  return await commands.getLastSelectedAgents();
}

/**
 * 获取 GUI scope-aware 默认安装目标
 */
export async function getDefaultTargetAgents(): Promise<DefaultTargetAgents | null> {
  return await commands.getDefaultTargetAgents();
}

export async function getDefaultTargetAgentsV2(
  context: ContextRef,
): Promise<DefaultTargetAgents | null> {
  return unwrap(await commands.getDefaultTargetAgentsV2(context));
}

/**
 * 保存 GUI scope-aware 默认安装目标
 */
export async function saveDefaultTargetAgents(defaults: DefaultTargetAgents): Promise<void> {
  unwrap(await commands.saveDefaultTargetAgents(defaults));
}

export async function saveDefaultTargetAgentsV2(
  context: ContextRef,
  defaults: DefaultTargetAgents,
): Promise<void> {
  unwrap(await commands.saveDefaultTargetAgentsV2(context, defaults));
}

// ============ 安装相关 API ============

/**
 * 从来源获取可用的 skills 列表
 */
export async function fetchAvailable(source: string): Promise<FetchResult> {
  return unwrap(await commands.fetchAvailable(source));
}

export async function fetchAvailableV2(
  context: ContextRef,
  source: string,
): Promise<FetchResult> {
  return unwrap(await commands.fetchAvailableV2(context, source));
}

/**
 * 安装选中的 skills
 */
export async function installSkills(params: InstallParams): Promise<InstallResults> {
  return unwrap(await commands.installSkills(params));
}

export async function installSkillsV2(
  context: ContextRef,
  params: InstallParams,
): Promise<InstallResults> {
  return unwrap(await commands.installSkillsV2(context, params));
}

/**
 * 检测覆盖情况
 */
export async function checkOverwrites(
  skills: string[],
  agents: string[],
  scope: Scope,
  projectPath?: string,
  privateCopyAgents: string[] = [],
  agentTargets: InstallTargetSpec[] = [],
): Promise<Partial<Record<string, string[]>>> {
  return unwrap(
    await commands.checkOverwrites(
      skills,
      agents,
      privateCopyAgents,
      scope,
      projectPath ?? null,
      agentTargets,
    )
  );
}

export async function checkOverwritesV2(
  context: ContextRef,
  skills: string[],
  agents: string[],
  privateCopyAgents: string[] = [],
  agentTargets: InstallTargetSpec[] = [],
): Promise<Partial<Record<string, string[]>>> {
  return unwrap(await commands.checkOverwritesV2(
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
export async function removeSkill(params: {
  scope: Scope;
  name: string;
  projectPath?: string;
  agents?: AgentType[];
  fullRemoval?: boolean;
  agentTargets?: InstallTargetSpec[];
}): Promise<RemoveResult> {
  return unwrap(
    await commands.removeSkill(
      params.scope,
      params.name,
      params.projectPath ?? null,
      params.agents ?? null,
      params.fullRemoval ?? null,
      params.agentTargets ?? null,
    )
  );
}

export async function removeSkillV2(
  context: ContextRef,
  params: {
    name: string;
    agents?: AgentType[];
    fullRemoval?: boolean;
    agentTargets?: InstallTargetSpec[];
  },
): Promise<RemoveResult> {
  return unwrap(await commands.removeSkillV2(
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
export async function getSkillAgentDetails(params: {
  scope: Scope;
  name: string;
  projectPath?: string;
}): Promise<SkillAgentDetails> {
  return unwrap(
    await commands.getSkillAgentDetails(params.scope, params.name, params.projectPath ?? null)
  );
}

export async function getSkillAgentDetailsV2(
  context: ContextRef,
  name: string,
): Promise<SkillAgentDetails> {
  return unwrap(await commands.getSkillAgentDetailsV2(context, name));
}

// ============ 项目管理 API ============

/**
 * 添加项目路径
 */
export async function addProject(path: string): Promise<string[]> {
  return unwrap(await commands.addProject(path));
}

/**
 * 移除项目路径
 */
export async function removeProject(path: string): Promise<string[]> {
  return unwrap(await commands.removeProject(path));
}

/**
 * 检查项目路径是否存在
 */
export async function checkProjectPath(path: string): Promise<boolean> {
  return await commands.checkProjectPath(path);
}

/**
 * 在系统文件管理器中打开路径
 */
export async function openInExplorer(path: string): Promise<void> {
  unwrap(await commands.openInExplorer(path));
}

// ============ Environment / mutation API ============

export async function listEnvironments(): Promise<EnvironmentInfo[]> {
  return unwrap(await commands.listEnvironmentsV2());
}

export async function connectEnvironment(distroName: string): Promise<WslSession> {
  return unwrap(await commands.connectEnvironmentV2(distroName));
}

export async function mapEnvironmentPath(
  environment: EnvironmentRef,
  path: string,
): Promise<string> {
  return unwrap(await commands.mapEnvironmentPathV2(environment, path));
}

export async function listEnvironmentProjects(
  environment: EnvironmentRef,
): Promise<ProjectBinding[]> {
  return unwrap(await commands.listEnvironmentProjectsV2(environment));
}

export async function addEnvironmentProject(
  environment: EnvironmentRef,
  nativePath: string,
): Promise<ProjectBinding[]> {
  return unwrap(await commands.addEnvironmentProjectV2(environment, nativePath));
}

export async function removeEnvironmentProject(
  environment: EnvironmentRef,
  projectId: string,
): Promise<ProjectBinding[]> {
  return unwrap(await commands.removeEnvironmentProjectV2(environment, projectId));
}

export async function setEnvironmentProjectCrossStorageWarning(
  environment: EnvironmentRef,
  projectId: string,
  suppressed: boolean,
): Promise<ProjectBinding[]> {
  return unwrap(await commands.setEnvironmentProjectCrossStorageWarningV2(
    environment,
    projectId,
    suppressed,
  ));
}

export async function getActiveMutation(): Promise<ActiveMutation | null> {
  return await commands.getActiveMutation();
}

export async function requestCancelActiveMutation(): Promise<boolean> {
  return unwrap(await commands.requestCancelActiveMutation());
}

// ============ 更新检测 API ============

/**
 * 检测指定 scope 的 skills 是否有更新
 */
export async function checkUpdates(
  scope: Scope,
  projectPath?: string
): Promise<SkillUpdateInfo[]> {
  return unwrap(await commands.checkUpdates(scope, projectPath ?? null));
}

export async function checkUpdatesV2(context: ContextRef): Promise<SkillUpdateInfo[]> {
  return unwrap(await commands.checkUpdatesV2(context));
}

/**
 * 更新指定 skill
 */
export async function updateSkill(params: {
  scope: Scope;
  name: string;
  projectPath?: string;
}): Promise<UpdateSkillResponse> {
  return unwrap(await commands.updateSkill(params.scope, params.name, params.projectPath ?? null));
}

export async function updateSkillV2(
  context: ContextRef,
  name: string,
): Promise<UpdateSkillResponse> {
  return unwrap(await commands.updateSkillV2(context, name));
}

/**
 * 批量更新多个 skills（同源 clone 合并）
 */
export async function updateSkillsBatch(params: {
  scope: Scope;
  names: string[];
  projectPath?: string;
}): Promise<UpdateSkillResponse> {
  return unwrap(await commands.updateSkillsBatch(params.scope, params.names, params.projectPath ?? null));
}

export async function updateSkillsBatchV2(
  context: ContextRef,
  names: string[],
): Promise<UpdateSkillResponse> {
  return unwrap(await commands.updateSkillsBatchV2(context, names));
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
  scope: string;
  projectPath?: string;
  prefillSource?: string;
  prefillSkillName?: string;
  context?: ContextRef;
}): Promise<void> {
  unwrap(
    await commands.openInstallWizard(
      params.entryPoint,
      params.scope,
      params.projectPath ?? null,
      params.prefillSource ?? null,
      params.prefillSkillName ?? null,
      params.context ?? null,
    )
  );
}

// ============ Agent 管理 API ============

/**
 * 管理 skill 的 agent 支持（添加/移除）
 */
export async function manageSkillAgents(params: {
  skillName: string;
  scope: Scope;
  projectPath?: string;
  addAgents: AgentType[];
  removeAgents: AgentType[];
  privateCopyAgents?: AgentType[];
  mode: InstallMode;
}): Promise<ManageAgentsResult> {
  return unwrap(
    await commands.manageSkillAgents(
      params.skillName,
      params.scope,
      params.projectPath ?? null,
      params.addAgents,
      params.removeAgents,
      params.privateCopyAgents ?? [],
      params.mode,
    )
  );
}

export async function manageSkillAgentsV2(
  context: ContextRef,
  params: {
    skillName: string;
    addAgents: AgentType[];
    removeAgents: AgentType[];
    privateCopyAgents?: AgentType[];
    mode: InstallMode;
  },
): Promise<ManageAgentsResult> {
  return unwrap(await commands.manageSkillAgentsV2(
    context,
    params.skillName,
    params.addAgents,
    params.removeAgents,
    params.privateCopyAgents ?? [],
    params.mode,
  ));
}

export async function cleanupDuplicateAgentCopy(params: {
  skillName: string;
  agent: AgentType;
  scope: Scope;
  projectPath?: string;
}): Promise<DuplicateCleanupResult> {
  return unwrap(
    await commands.cleanupDuplicateAgentCopy(
      params.skillName,
      params.agent,
      params.scope,
      params.projectPath ?? null,
    )
  );
}

export async function cleanupDuplicateAgentCopies(params: {
  skillName: string;
  scope: Scope;
  projectPath?: string;
  agents: AgentType[];
}): Promise<DuplicateCleanupResult[]> {
  return unwrap(
    await commands.cleanupDuplicateAgentCopies(
      params.skillName,
      params.scope,
      params.projectPath ?? null,
      params.agents,
    )
  );
}

export async function cleanupDuplicateAgentCopiesV2(
  context: ContextRef,
  params: { skillName: string; agents: AgentType[] },
): Promise<DuplicateCleanupResult[]> {
  return unwrap(await commands.cleanupDuplicateAgentCopiesV2(
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
  sourceProjectPath: string;
  targetProjectPaths: string[];
  agents: string[];
  privateCopyAgents?: string[];
}): Promise<CopySkillResult> {
  return unwrap(
    await commands.copySkillToProjects(
      params.skillName,
      params.sourceProjectPath,
      params.targetProjectPaths,
      params.agents,
      params.privateCopyAgents ?? [],
    )
  );
}

export async function copySkillToProjectsV2(params: {
  skillName: string;
  source: ContextRef;
  targets: ContextRef[];
  agents: string[];
  privateCopyAgents?: string[];
}): Promise<CopySkillResult> {
  return unwrap(await commands.copySkillToProjectsV2(
    params.skillName,
    params.source,
    params.targets,
    params.agents,
    params.privateCopyAgents ?? [],
  ));
}

/**
 * 检查 skill 在哪些项目中已存在
 */
export async function checkSkillInProjects(
  skillName: string,
  projectPaths: string[],
): Promise<ProjectSkillStatus[]> {
  return await commands.checkSkillInProjects(skillName, projectPaths);
}
