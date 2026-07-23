import { describe, expect, it, vi } from 'vitest';
import { repairSkillSource } from '../skill-repair';

const context = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
} as const;

function request(stopRequested = () => false) {
  return {
    context,
    source: 'owner/repo',
    skillName: 'toolkit',
    agents: ['claude-code'],
    privateAdaptedAgents: ['claude-code'],
    privateCopyAgents: [],
    acknowledgeRisk: true,
    operationId: 'repair-1',
    stopRequested,
  };
}

function api() {
  return {
    fetchAvailable: vi.fn().mockResolvedValue({
      discoverySession: { sessionId: 'discovery-1' },
      riskPolicy: { kind: 'none', code: null },
      skills: [{ name: 'toolkit', relativePath: 'skills/toolkit' }],
    }),
    prepareInstall: vi.fn().mockResolvedValue({
      status: 'ready',
      prepared: {
        request: { context },
        preview: { token: { generation: 'preview-1' } },
      },
    }),
    installSkills: vi.fn().mockResolvedValue({
      units: [{ unitId: 'toolkit', status: 'succeeded' }],
    }),
  };
}

describe('repairSkillSource', () => {
  it('returns a typed success after discovery, preparation, and execution', async () => {
    const workflowApi = api();

    await expect(repairSkillSource(request(), workflowApi as never)).resolves.toEqual({
      status: 'succeeded',
      response: { units: [{ unitId: 'toolkit', status: 'succeeded' }] },
    });
    expect(workflowApi.fetchAvailable).toHaveBeenCalledWith(context, 'owner/repo', 'repair-1');
  });

  it('preserves a partial execution response for the dialog to present', async () => {
    const workflowApi = api();
    workflowApi.installSkills.mockResolvedValue({
      units: [{ unitId: 'toolkit', status: 'failed' }],
    });

    await expect(repairSkillSource(request(), workflowApi as never)).resolves.toEqual({
      status: 'partial',
      response: { units: [{ unitId: 'toolkit', status: 'failed' }] },
    });
  });

  it('stops before preparation when cancellation is requested after discovery', async () => {
    const workflowApi = api();
    const stopRequested = vi.fn()
      .mockReturnValueOnce(false)
      .mockReturnValue(true);

    await expect(repairSkillSource(request(stopRequested), workflowApi as never))
      .resolves.toEqual({ status: 'stopped' });
    expect(workflowApi.prepareInstall).not.toHaveBeenCalled();
  });
});
