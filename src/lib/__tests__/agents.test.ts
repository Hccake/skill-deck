import { describe, expect, it } from 'vitest';
import type {
  AgentRuntimeSnapshot,
  AgentSource,
  DetectionState,
  ResolvedAgent,
  ResolvedAgentScope,
} from '@/bindings';
import {
  agentDisplayName,
  agentId,
  agentsForScope,
  isAgentDetected,
  isAgentSelectable,
} from '../agents';

function resolvedScope(): ResolvedAgentScope {
  return {
    enabled: true,
    readsStandard: true,
    standardPath: '/home/alice/.agents/skills',
    privatePath: null,
    readPaths: ['/home/alice/.agents/skills'],
    standardPresence: 'present',
    privatePresence: null,
    legacyPaths: [],
  };
}

function resolvedAgent(
  id: string,
  source: AgentSource,
  detection: DetectionState = 'detected',
): ResolvedAgent {
  return {
    definition: {
      id,
      displayName: id,
      source,
      aliases: [],
      global: { enabled: true, readsStandard: true, privatePath: null },
      project: { enabled: true, readsStandard: true, privatePath: null },
      detection: {
        kind: 'anyPathExists',
        paths: [{ kind: 'home', relativePath: `.${id}` }],
      },
      legacyPaths: [],
      adapter: 'standard',
    },
    detection,
    detectionReason: null,
    global: resolvedScope(),
    project: resolvedScope(),
  };
}

function runtimeSnapshot(agents: ResolvedAgent[]): AgentRuntimeSnapshot {
  return {
    registryRevision: 'registry-1',
    environmentRevision: 'environment-1',
    environment: { kind: 'native' },
    availability: 'available',
    projectPath: null,
    agents: Object.fromEntries(agents.map((agent) => [agentId(agent), agent])),
  };
}

describe('Agent runtime projections', () => {
  it('keeps built-in and custom agents in one scope list', () => {
    const snapshot = runtimeSnapshot([
      resolvedAgent('codex', 'builtin'),
      resolvedAgent('my-agent', 'custom'),
    ]);

    expect(agentsForScope(snapshot, 'global').map(agentId)).toEqual([
      'codex',
      'my-agent',
    ]);
  });

  it('keeps an explicitly selected indeterminate agent selectable', () => {
    const agent = resolvedAgent('my-agent', 'custom', 'indeterminate');

    expect(agentDisplayName(agent)).toBe('my-agent');
    expect(isAgentDetected(agent)).toBe(false);
    expect(isAgentSelectable(agent)).toBe(true);
  });
});
