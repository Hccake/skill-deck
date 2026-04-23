// src/hooks/useTauriApi.ts
// 使用 tauri-specta 生成的类型安全绑定
import { commands } from '@/bindings';
import type {
  AgentInfo,
  AgentType,
  ListSkillsResult,
  SkillScope,
  RemoveResult,
  SkillUpdateInfo,
  UpdateSkillResponse,
  FetchResult as RawFetchResult,
  InstallMode,
  InstallParams as RawInstallParams,
  InstallResults,
  SkillDeckConfig as RawSkillDeckConfig,
  Scope,
  SkillAuditData,
  SkillAgentDetails,
  ManageAgentsResult,
  CopySkillResult,
  CopyProjectResult,
  ProjectSkillStatus,
} from '@/bindings';

export interface InstallRiskPolicy {
  kind: 'none' | 'require-confirmation';
  code?: string | null;
}

export type FetchResult = RawFetchResult & {
  riskPolicy?: InstallRiskPolicy | null;
};

export type InstallParams = RawInstallParams & {
  acknowledgeRisk?: boolean;
};

export type SkillDeckConfig = RawSkillDeckConfig & {
  gitCloneTimeoutSecs?: number;
};

// 重导出类型供组件使用
export type { AgentInfo, AgentType, ListSkillsResult, SkillScope, RemoveResult, SkillUpdateInfo, UpdateSkillResponse, InstallMode, InstallResults, SkillAuditData, SkillAgentDetails, ManageAgentsResult, CopySkillResult, CopyProjectResult, ProjectSkillStatus };

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
 * 列出已安装的 Skills
 */
export async function listSkills(params?: ListSkillsParams): Promise<ListSkillsResult> {
  return unwrap(await commands.listSkills({
    scope: params?.scope ?? null,
    projectPath: params?.projectPath ?? null,
  }));
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
  return unwrap(await commands.getConfig()) as SkillDeckConfig;
}

/**
 * 保存应用配置
 */
export async function saveConfig(config: SkillDeckConfig): Promise<void> {
  unwrap(await commands.saveConfig(config as RawSkillDeckConfig));
}

// ============ Agent 选择相关 API ============

/**
 * 获取上次选择的 agents
 */
export async function getLastSelectedAgents(): Promise<string[]> {
  return await commands.getLastSelectedAgents();
}

/**
 * 保存选择的 agents
 */
export async function saveLastSelectedAgents(agents: string[]): Promise<void> {
  unwrap(await commands.saveLastSelectedAgents(agents));
}

// ============ 安装相关 API ============

/**
 * 从来源获取可用的 skills 列表
 */
export async function fetchAvailable(source: string): Promise<FetchResult> {
  return unwrap(await commands.fetchAvailable(source)) as FetchResult;
}

/**
 * 安装选中的 skills
 */
export async function installSkills(params: InstallParams): Promise<InstallResults> {
  return unwrap(await commands.installSkills(params as RawInstallParams));
}

/**
 * 检测覆盖情况
 */
export async function checkOverwrites(
  skills: string[],
  agents: string[],
  scope: Scope,
  projectPath?: string
): Promise<Partial<Record<string, string[]>>> {
  return unwrap(await commands.checkOverwrites(skills, agents, scope, projectPath ?? null));
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
}): Promise<RemoveResult> {
  return unwrap(
    await commands.removeSkill(
      params.scope,
      params.name,
      params.projectPath ?? null,
      params.agents ?? null,
      params.fullRemoval ?? null,
    )
  );
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
}): Promise<void> {
  unwrap(
    await commands.openInstallWizard(
      params.entryPoint,
      params.scope,
      params.projectPath ?? null,
      params.prefillSource ?? null,
      params.prefillSkillName ?? null
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
  mode: InstallMode;
}): Promise<ManageAgentsResult> {
  return unwrap(
    await commands.manageSkillAgents(
      params.skillName,
      params.scope,
      params.projectPath ?? null,
      params.addAgents,
      params.removeAgents,
      params.mode,
    )
  );
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
}): Promise<CopySkillResult> {
  return unwrap(
    await commands.copySkillToProjects(
      params.skillName,
      params.sourceProjectPath,
      params.targetProjectPaths,
      params.agents,
    )
  );
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
