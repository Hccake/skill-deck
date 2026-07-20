import {
  acquireSelectedPayloads,
  previewInstall,
} from '@/hooks/useTauriApi';
import type {
  AgentWriteIntent,
  AppError,
  ContextRef,
  DiscoverySessionHandle,
  InstallMode,
  InstallPreview,
  InstallRequest,
} from '@/bindings';
import { toAppError } from '@/utils/to-app-error';

export interface InstallPreparationInput {
  context: ContextRef;
  source: string;
  discoverySession: DiscoverySessionHandle;
  skillPaths: string[];
  skills: string[];
  agentIntents: AgentWriteIntent[];
  requestedMode: InstallMode;
  acknowledgeRisk: boolean;
}

export interface PreparedInstall {
  request: InstallRequest;
  preview: InstallPreview;
}

export interface InstallPreparationApi {
  acquireSelectedPayloads: typeof acquireSelectedPayloads;
  previewInstall: typeof previewInstall;
}

export type InstallPreparationOutcome =
  | { status: 'ready'; prepared: PreparedInstall }
  | { status: 'failed'; stage: 'payload' | 'preview'; error: AppError };

const defaultApi: InstallPreparationApi = {
  acquireSelectedPayloads,
  previewInstall,
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
    agentIntents: input.agentIntents,
    requestedMode: input.requestedMode,
    acknowledgeRisk: input.acknowledgeRisk,
  };

  try {
    const preview = await api.previewInstall(request);
    return { status: 'ready', prepared: { request, preview } };
  } catch (error) {
    return { status: 'failed', stage: 'preview', error: toAppError(error) };
  }
}
