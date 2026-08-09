// src/components/skills/add-skill/types.ts

import type {
  AgentId,
  AppError,
  AvailableSkill,
  DiscoverySessionHandle,
  InstallResponse,
  SkillLocationRef,
} from '@/bindings';
import type { InstallRiskPolicy } from '@/hooks/useTauriApi';
import type {
  InstallPreparationOutcome,
} from '@/workflows/skill-install-preparation';

/** 安装错误详情（UI 视图模型，由 parseInstallError 从 AppError 转换而来） */
export interface InstallError {
  message: string;
  details?: string;
  suggestions?: string[];
}

export type InstallPreparationState =
  | { status: 'idle' | 'preparing' }
  | InstallPreparationOutcome;

/** 安装入口类型 */
export type EntryPoint = 'skills-panel' | 'discovery';

/** 核心步骤（用户需要操作的 5 步） */
export type CoreStep = 'scope' | 'source' | 'skills' | 'options' | 'confirm';

/** 结果态步骤 */
type ResultStep = 'installing' | 'complete' | 'error';

/** 所有向导步骤 */
export type WizardStep = CoreStep | ResultStep;

const DISCOVERY_STEP_FLOW: CoreStep[] = ['scope', 'source', 'skills', 'options', 'confirm'];
const CONTEXT_STEP_FLOW: CoreStep[] = ['source', 'skills', 'options', 'confirm'];

/** 获取步骤流程 */
export function getStepFlow(entryPoint: EntryPoint = 'skills-panel'): CoreStep[] {
  return entryPoint === 'discovery' ? DISCOVERY_STEP_FLOW : CONTEXT_STEP_FLOW;
}

/** AddSkillWizard 内部状态 */
export interface WizardState {
  step: WizardStep;
  entryPoint: EntryPoint;

  // Scope
  scope: 'global' | 'project';
  projectPath?: string;
  context: SkillLocationRef;
  environmentName?: string;

  // Source
  source: string;
  fetchStatus: 'idle' | 'loading' | 'error' | 'success';
  fetchError: AppError | null;
  gitRef: string | null;
  discoverySession?: DiscoverySessionHandle;
  riskPolicy: InstallRiskPolicy | null;
  riskAcknowledged: boolean;

  // Skills
  availableSkills: AvailableSkill[];
  selectedSkills: string[];
  skillFilter: string | null;
  skillSearchQuery: string;

  // Confirm
  overwrites: Record<string, string[]>;
  preparation: InstallPreparationState;

  // CLI 预填值
  preSelectedSkills: string[];
  preSelectedAgents: AgentId[];

  // Installing
  installResults: InstallResponse | null;
  installError?: InstallError;
}

export function canProceedForStep(state: WizardState): boolean {
  switch (state.step) {
    case 'source':
      return state.fetchStatus === 'success' && state.availableSkills.length > 0;
    case 'scope':
      return true;
    case 'skills':
      return state.selectedSkills.length > 0;
    case 'options':
      return true;
    case 'confirm':
      return state.preparation.status === 'ready'
        && (state.riskPolicy?.kind !== 'require-confirmation' || state.riskAcknowledged);
    default:
      return false;
  }
}
