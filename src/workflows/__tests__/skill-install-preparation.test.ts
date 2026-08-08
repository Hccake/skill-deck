import { describe, expect, it, vi } from 'vitest';
import type { InstallPreview, InstallRequest } from '@/bindings';
import {
  prepareInstall,
  type InstallPreparationApi,
  type InstallPreparationInput,
} from '../skill-install-preparation';

const input: InstallPreparationInput = {
  context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
  source: 'owner/repo',
  discoverySession: {
    sessionId: 'discovery-1',
    environment: { kind: 'native' },
    sourceFingerprint: 'source-1',
    expiresAtEpochMs: 1000,
  },
  skillPaths: ['skills/demo/SKILL.md'],
  skills: ['demo'],
  explicitAgentIds: ['cursor'],
  agentSelection: {
    revision: 'selection-1',
    selectedOptionIds: ['cursor'],
    requestedMode: 'copy',
  },
  acknowledgeRisk: true,
};

function api(overrides: Partial<InstallPreparationApi> = {}): InstallPreparationApi {
  return {
    acquireSelectedPayloads: vi.fn().mockResolvedValue([
      { sessionId: 'discovery-1', skillPath: 'skills/demo/SKILL.md' },
    ]),
    previewInstall: vi.fn().mockResolvedValue({
      status: 'ready',
      preview: {
        token: {
          generation: 'preview-1',
          registryRevision: 'registry-1',
          environmentRevision: 'environment-1',
          contextRevision: 'context-1',
        },
        skills: [],
      } satisfies InstallPreview,
    }),
    getInstallAgentSelection: vi.fn(),
    ...overrides,
  };
}

describe('prepareInstall', () => {
  it('returns a payload-stage failure without calling preview', async () => {
    const error = { kind: 'stalePayload', data: {} } as never;
    const preparationApi = api({
      acquireSelectedPayloads: vi.fn().mockRejectedValue(error),
    });

    await expect(prepareInstall(input, preparationApi)).resolves.toEqual({
      status: 'failed',
      stage: 'payload',
      error,
    });
    expect(preparationApi.previewInstall).not.toHaveBeenCalled();
  });

  it('returns a preview-stage failure with the original error', async () => {
    const error = { kind: 'staleTarget', data: {} } as never;
    const preparationApi = api({
      previewInstall: vi.fn().mockRejectedValue(error),
    });

    await expect(prepareInstall(input, preparationApi)).resolves.toEqual({
      status: 'failed',
      stage: 'preview',
      error,
    });
  });

  it('returns one immutable prepared request and its preview', async () => {
    const preview = { token: { generation: 'preview-1' }, skills: [] } as unknown as InstallPreview;
    const preparationApi = api({ previewInstall: vi.fn().mockResolvedValue({ status: 'ready', preview }) });

    const result = await prepareInstall(input, preparationApi);

    expect(result).toEqual({
      status: 'ready',
      prepared: {
        request: expect.objectContaining<Partial<InstallRequest>>({
          context: input.context,
          source: input.source,
          skills: input.skills,
          agentSelection: input.agentSelection,
          acknowledgeRisk: true,
        }),
        preview,
      },
    });
    expect(preparationApi.acquireSelectedPayloads).toHaveBeenCalledOnce();
    expect(preparationApi.previewInstall).toHaveBeenCalledOnce();
  });

  it('reloads a complete selection snapshot when the submitted revision is stale', async () => {
    const staleSnapshot = {
      selection: {
        agents: [], installOptions: [], groups: [], initialSelectedOptionIds: [],
        unavailableExplicitAgents: [], userModeOptionIds: [], revision: 'selection-2',
      },
      defaultSelectionWarning: null,
    } as const;
    const latestSnapshot = {
      ...staleSnapshot,
      selection: {
        ...staleSnapshot.selection,
        initialSelectedOptionIds: ['new-item'],
        revision: 'selection-3',
      },
      defaultSelectionWarning: 'readFailed' as const,
    };
    const preparationApi = api({
      previewInstall: vi.fn().mockResolvedValue({ status: 'selectionStale', snapshot: staleSnapshot }),
      getInstallAgentSelection: vi.fn().mockResolvedValue(latestSnapshot),
    });

    await expect(prepareInstall(input, preparationApi)).resolves.toEqual({
      status: 'selectionStale',
      snapshot: latestSnapshot,
    });
    expect(preparationApi.getInstallAgentSelection).toHaveBeenCalledWith(
      input.context,
      input.explicitAgentIds,
    );
  });
});
