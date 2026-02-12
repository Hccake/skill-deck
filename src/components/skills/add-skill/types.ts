// src/components/skills/add-skill/types.ts

import type { AgentInfo, AppError, AvailableSkill, InstallMode, InstallResults } from '@/bindings';

/** 安装错误详情（UI 视图模型，由 parseInstallError 从 AppError 转换而来） */
export interface InstallError {
  message: string;
  details?: string;
  suggestions?: string[];
}

/** 安装入口类型 */
export type EntryPoint = 'skills-panel' | 'discovery';

/** 核心步骤（用户需要操作的 5 步） */
export type CoreStep = 'scope' | 'source' | 'skills' | 'options' | 'confirm';

/** 结果态步骤 */
export type ResultStep = 'installing' | 'complete' | 'error';

/** 所有向导步骤 */
export type WizardStep = CoreStep | ResultStep;

/** 固定 5 步流程（所有入口统一） */
const STEP_FLOW: CoreStep[] = ['scope', 'source', 'skills', 'options', 'confirm'];

/** 获取步骤流程 */
export function getStepFlow(_entryPoint?: EntryPoint): CoreStep[] {
  return STEP_FLOW;
}

/** 向导初始化参数（通过窗口 URL query 传递） */
export interface WizardParams {
  entryPoint: EntryPoint;
  scope: 'global' | 'project';
  projectPath?: string;
  /** Discovery 入口的预填信息 */
  prefillSource?: string;
  prefillSkillName?: string;
}

/** AddSkillWizard 内部状态 */
export interface WizardState {
  step: WizardStep;
  entryPoint: EntryPoint;

  // Scope
  scope: 'global' | 'project';
  projectPath?: string;

  // Source
  source: string;
  fetchStatus: 'idle' | 'loading' | 'error' | 'success';
  fetchError: AppError | null;

  // Skills
  availableSkills: AvailableSkill[];
  selectedSkills: string[];
  skillFilter: string | null;
  skillSearchQuery: string;

  // Options
  selectedAgents: string[];
  allAgents: AgentInfo[];
  agentsCollapsed: boolean;
  mode: InstallMode;
  otherAgentsExpanded: boolean;
  otherAgentsSearchQuery: string;

  // Confirm
  overwrites: Record<string, string[]>;

  // CLI 预填值
  preSelectedSkills: string[];
  preSelectedAgents: string[];

  // Installing
  installResults: InstallResults | null;
  installError?: InstallError;
}
