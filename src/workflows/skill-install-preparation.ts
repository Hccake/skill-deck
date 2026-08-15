import {
  acquireSelectedPayloads,
  getInstallAgentSelection,
  previewInstall,
} from '@/hooks/useTauriApi';
import type {
  AgentSelectionSubmission,
  AppError,
  SkillLocationRef,
  DiscoverySessionHandle,
  InstallPreview,
  InstallRequest,
} from '@/bindings';
import { toAppError } from '@/utils/to-app-error';

export interface InstallPreparationInput {
  context: SkillLocationRef;
  source: string;
  discoverySession: DiscoverySessionHandle;
  skillPaths: string[];
  skills: string[];
  explicitAgentIds: import('@/bindings').AgentId[];
  agentSelection: AgentSelectionSubmission;
  acknowledgeRedirect: boolean;
}

export interface PreparedInstall {
  request: InstallRequest;
  preview: InstallPreview;
}

export interface InstallPreparationApi {
  acquireSelectedPayloads: typeof acquireSelectedPayloads;
  previewInstall: typeof previewInstall;
  getInstallAgentSelection: typeof getInstallAgentSelection;
}

export type InstallPreparationOutcome =
  | { status: 'ready'; prepared: PreparedInstall }
  | { status: 'selectionStale'; snapshot: import('@/bindings').InstallAgentSelectionSnapshot }
  | { status: 'failed'; stage: 'payload' | 'preview'; error: AppError };

const defaultApi: InstallPreparationApi = {
  acquireSelectedPayloads,
  previewInstall,
  getInstallAgentSelection,
};

/**
 * 固定一次安装意图对应的 payload 和 preview。调用方只接收 ready 或带阶段的失败，
 * 不会出现 request 和 preview 分别存在、彼此失配的中间状态。
 */
export async function prepareInstall(
  input: InstallPreparationInput,
  api: InstallPreparationApi = defaultApi,
): Promise<InstallPreparationOutcome> {
  let payloads;
  try {
    payloads = await api.acquireSelectedPayloads({
      discoverySession: input.discoverySession,
      skillPaths: input.skillPaths,
    });
  } catch (error) {
    return { status: 'failed', stage: 'payload', error: toAppError(error) };
  }

  const request: InstallRequest = {
    context: input.context,
    source: input.source,
    discoverySession: input.discoverySession,
    payloads,
    skills: input.skills,
    agentSelection: input.agentSelection,
    acknowledgeRedirect: input.acknowledgeRedirect,
  };

  try {
    const outcome = await api.previewInstall(request);
    if (outcome.status === 'selectionStale') {
      const snapshot = await api.getInstallAgentSelection(input.context, input.explicitAgentIds);
      return { status: 'selectionStale', snapshot };
    }
    return { status: 'ready', prepared: { request, preview: outcome.preview } };
  } catch (error) {
    return { status: 'failed', stage: 'preview', error: toAppError(error) };
  }
}
