import { describe, expect, it } from 'vitest';
import type { ResolvedAgent } from '@/bindings';
import {
  buildAgentWriteIntents,
  hasFailedMutationUnits,
  type AdapterTargetSelection,
} from '../install-workflow';

function resolvedAgent(
  id: string,
  options: { readsShared: boolean; privatePath: string | null },
): ResolvedAgent {
  const scope = {
    enabled: true,
    readsShared: options.readsShared,
    sharedPath: options.readsShared ? '/shared' : null,
    privatePath: options.privatePath,
    readPaths: [],
    sharedPresence: null,
    privatePresence: null,
    legacyPaths: [],
  };
  return {
    definition: {
      id,
      displayName: id,
      source: 'builtin',
      aliases: [],
      global: { enabled: true, readsShared: options.readsShared, privatePath: null },
      project: { enabled: true, readsShared: options.readsShared, privatePath: null },
      detection: { kind: 'anyPathExists', paths: [] },
      legacyPaths: [],
      adapter: 'standard',
    },
    detection: 'detected',
    detectionReason: null,
    global: scope,
    project: scope,
  };
}

describe('install workflow model', () => {
  it('creates one intent per Agent and keeps installation mode out of Agent state', () => {
    const agents = [
      resolvedAgent('shared-agent', { readsShared: true, privatePath: '/shared-agent' }),
      resolvedAgent('private-agent', { readsShared: false, privatePath: '/private-agent' }),
      resolvedAgent('eve', { readsShared: true, privatePath: '/eve' }),
    ];
    const adapterTargets: AdapterTargetSelection[] = [
      { agentId: 'eve', targetId: 'eve:root' },
      { agentId: 'eve', targetId: 'eve:subagent:research' },
    ];

    expect(buildAgentWriteIntents({
      agents,
      scope: 'project',
      selectedAgents: ['private-agent'],
      privateCopyAgents: ['shared-agent'],
      adapterTargets,
    })).toEqual([
      {
        agentId: 'eve',
        privateEntry: 'none',
        adapterTargets: ['eve:root', 'eve:subagent:research'],
      },
      {
        agentId: 'private-agent',
        privateEntry: 'required',
        adapterTargets: [],
      },
      {
        agentId: 'shared-agent',
        privateEntry: 'optionalSelected',
        adapterTargets: [],
      },
    ]);
  });

  it('treats every non-succeeded mutation unit as an incomplete install', () => {
    expect(hasFailedMutationUnits({ units: [] })).toBe(false);
    expect(hasFailedMutationUnits({
      units: [{ status: 'notRun' } as never],
    })).toBe(true);
  });
});
