// src/components/skills/add-skill/types.ts

/** 可用的 Skill（从仓库获取） */
export interface AvailableSkill {
  name: string;
  description: string;
  relativePath: string;
}

/** fetch_available 返回结果 */
export interface FetchResult {
  sourceType: string;
  sourceUrl: string;
  skillFilter: string | null;
  skills: AvailableSkill[];
}

/** 安装参数 */
export interface InstallParams {
  source: string;
  skills: string[];
  agents: string[];
  scope: 'global' | 'project';
  projectPath?: string;
  mode: 'symlink' | 'copy';
}

/** 单个安装结果 */
export interface InstallResult {
  skillName: string;
  agent: string;
  success: boolean;
  path: string;
  canonicalPath?: string;
  mode: 'symlink' | 'copy';
  symlinkFailed: boolean;
  error?: string;
}

/** 安装结果汇总 */
export interface InstallResults {
  successful: InstallResult[];
  failed: InstallResult[];
  symlinkFallbackAgents: string[];
}

/** 安装错误详情 */
export interface InstallError {
  /** 用户友好的错误描述 */
  message: string;
  /** 详细上下文信息（可选） */
  details?: string;
  /** 修复建议列表（可选） */
  suggestions?: string[];
}

/** Agent 分组 */
export type AgentGroupId = 'universal' | 'detected' | 'other';

/** Agent 项 */
export interface AgentItem {
  id: string;
  displayName: string;
  detected: boolean;
  /** 是否是 Universal Agent（安装逻辑用） */
  isUniversal: boolean;
  /** 是否在 Universal 列表显示（UI 显示用） */
  showInUniversalList: boolean;
}

/** 向导步骤 */
export type WizardStep = 1 | 2 | 3 | 4 | 'installing' | 'complete' | 'error';

/** AddSkillDialog 状态 */
export interface AddSkillState {
  step: WizardStep;

  // Step 1: Source
  source: string;
  fetchStatus: 'idle' | 'loading' | 'error' | 'success';
  fetchError: string | null;

  // Step 2: Skills
  availableSkills: AvailableSkill[];
  selectedSkills: string[];
  skillFilter: string | null;
  skillSearchQuery: string;

  // Step 3: Options
  selectedAgents: string[];
  allAgents: AgentItem[];
  agentsCollapsed: boolean;
  mode: 'symlink' | 'copy';
  otherAgentsExpanded: boolean;
  otherAgentsSearchQuery: string;

  // Step 4: Confirm
  overwrites: Record<string, string[]>;

  // 从 CLI 命令解析出的预填值
  preSelectedSkills: string[];
  preSelectedAgents: string[];

  // Installing
  installResults: InstallResults | null;
  installError?: InstallError;
}
