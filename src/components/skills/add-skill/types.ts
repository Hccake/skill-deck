// src/components/skills/add-skill/types.ts

import type {AgentInfo, AppError, AvailableSkill, InstallMode, InstallResults} from '@/bindings';

/** 安装错误详情（UI 视图模型，由 parseInstallError 从 AppError 转换而来） */
export interface InstallError {
  /** 用户友好的错误描述 */
  message: string;
  /** 详细上下文信息（可选） */
  details?: string;
  /** 修复建议列表（可选） */
  suggestions?: string[];
}

/** 向导步骤 */
export type WizardStep = 1 | 2 | 3 | 4 | 'installing' | 'complete' | 'error';

/** AddSkillDialog 状态 */
export interface AddSkillState {
  step: WizardStep;

  // Step 1: Source
  source: string;
  fetchStatus: 'idle' | 'loading' | 'error' | 'success';
  fetchError: AppError | null;

  // Step 2: Skills
  availableSkills: AvailableSkill[];
  selectedSkills: string[];
  skillFilter: string | null;
  skillSearchQuery: string;

  // Step 3: Options
  selectedAgents: string[];
  allAgents: AgentInfo[];
  agentsCollapsed: boolean;
  mode: InstallMode;
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
