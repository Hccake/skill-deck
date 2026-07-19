import { describe, expect, it } from 'vitest';
import type { ResolvedAgent, ResolvedAgentScope } from '@/bindings';
import { makeResolvedAgent } from '@/test-utils';
import {
  canCreatePrivateCopy,
  filterAdditionalAgentIds,
  formatAgentTargetPath,
  getAgentDisplayPath,
  getAgentInstallPath,
  getSharedSkillDirectory,
  groupAgentsByScopedTarget,
  migrateDefaultTargetAgents,
} from '../agentTargets';

function makeAgent(
  id: string,
  globalReadsShared: boolean,
  projectReadsShared: boolean,
  detection: ResolvedAgent['detection'] = 'detected',
  targetOverrides: {
    global?: Partial<ResolvedAgentScope>;
    project?: Partial<ResolvedAgentScope>;
  } = {},
): ResolvedAgent {
  return makeResolvedAgent({
    id,
    detection,
    global: {
      readsShared: globalReadsShared,
      sharedPath: '/home/alice/.agents/skills',
      privatePath: globalReadsShared ? null : `/home/alice/.${id}/skills`,
      readPaths: globalReadsShared
        ? ['/home/alice/.agents/skills']
        : [`/home/alice/.${id}/skills`],
      ...targetOverrides.global,
    },
    project: {
      readsShared: projectReadsShared,
      sharedPath: '/work/app/.agents/skills',
      privatePath: projectReadsShared ? null : `/work/app/.${id}/skills`,
      readPaths: projectReadsShared
        ? ['/work/app/.agents/skills']
        : [`/work/app/.${id}/skills`],
      ...targetOverrides.project,
    },
  });
}

describe('agent target helpers', () => {
  it('provides normalized shared skill directory display paths', () => {
    expect(getSharedSkillDirectory('global')).toBe('~/.agents/skills');
    expect(getSharedSkillDirectory('project')).toBe('./.agents/skills');
  });

  it('filters shared-reading agents from additional defaults per scope', () => {
    const agents = [
      makeAgent('antigravity', false, true),
      makeAgent('warp', true, true),
      makeAgent('claude-code', false, false),
    ];

    expect(filterAdditionalAgentIds(['antigravity', 'warp', 'claude-code'], agents, 'global'))
      .toEqual(['antigravity', 'claude-code']);
    expect(filterAdditionalAgentIds(['antigravity', 'warp', 'claude-code'], agents, 'project'))
      .toEqual(['claude-code']);
  });

  it('migrates lastSelectedAgents independently for global and project', () => {
    const agents = [
      makeAgent('antigravity', false, true),
      makeAgent('codex', false, true),
      makeAgent('claude-code', false, false),
    ];

    expect(migrateDefaultTargetAgents(['antigravity', 'codex', 'claude-code'], agents))
      .toEqual({
        global: ['antigravity', 'codex', 'claude-code'],
        project: ['claude-code'],
      });
  });

  it('drops disabled and unknown agent ids', () => {
    const disabled = makeAgent('disabled', false, false, 'detected', {
      global: { enabled: false },
    });

    expect(filterAdditionalAgentIds(['disabled', 'missing'], [disabled], 'global'))
      .toEqual([]);
  });

  it('groups scoped targets by detection, saved selection, and private requirement', () => {
    const sharedDetected = makeAgent('shared-detected', true, true);
    const sharedUndetected = makeAgent('shared-undetected', true, true, 'notDetected');
    const privateDetected = makeAgent('private-detected', false, false);
    const privateSelected = makeAgent('private-selected', false, false, 'indeterminate');
    const privateHidden = makeAgent('private-hidden', false, false, 'notDetected');
    const disabled = makeAgent('disabled', false, false, 'detected', {
      global: { enabled: false },
    });

    const groups = groupAgentsByScopedTarget(
      [
        sharedDetected,
        sharedUndetected,
        privateDetected,
        privateSelected,
        privateHidden,
        disabled,
      ],
      'global',
      new Set(['private-selected']),
    );

    expect(groups.detectedAutomatic.map((agent) => agent.definition.id))
      .toEqual(['shared-detected']);
    expect(groups.undetectedAutomatic.map((agent) => agent.definition.id))
      .toEqual(['shared-undetected']);
    expect(groups.detectedSelectableAgents.map((agent) => agent.definition.id))
      .toEqual(['private-detected']);
    expect(groups.visibleSelectableAgents.map((agent) => agent.definition.id))
      .toEqual(['private-detected', 'private-selected']);
    expect(groups.hiddenSelectableAgents.map((agent) => agent.definition.id))
      .toEqual(['private-hidden']);
    expect(groups.selectableCount).toBe(3);
  });

  it('keeps undetected user-defined agents in the primary workflow groups', () => {
    const sharedCustom = makeResolvedAgent({
      id: 'shared-custom',
      source: 'custom',
      detection: 'notDetected',
      global: {
        readsShared: true,
        sharedPath: '/home/alice/.agents/skills',
        privatePath: '/home/alice/.shared-custom/skills',
      },
    });
    const privateCustom = makeResolvedAgent({
      id: 'private-custom',
      source: 'custom',
      detection: 'indeterminate',
      global: {
        readsShared: false,
        sharedPath: '/home/alice/.agents/skills',
        privatePath: '/home/alice/.private-custom/skills',
      },
    });

    const groups = groupAgentsByScopedTarget(
      [sharedCustom, privateCustom],
      'global',
    );

    expect(groups.visibleDefaultAvailableAgents.map((agent) => agent.definition.id))
      .toEqual(['shared-custom']);
    expect(groups.hiddenDefaultAvailableAgents).toEqual([]);
    expect(groups.visiblePrivateRequiredAgents.map((agent) => agent.definition.id))
      .toEqual(['private-custom']);
    expect(groups.hiddenPrivateRequiredAgents).toEqual([]);
    expect(groups.notDetectedDefaultAvailable).toEqual([sharedCustom]);
    expect(groups.indeterminateDefaultAvailable).toEqual([]);
  });

  it('derives display and install paths for Shared, Private, and Both', () => {
    const shared = makeAgent('shared', true, true);
    const privateAgent = makeAgent('private', false, false);
    const both = makeAgent('both', true, true, 'detected', {
      global: {
        privatePath: '/home/alice/.both/skills',
        readPaths: [
          '/home/alice/.agents/skills',
          '/home/alice/.both/skills',
        ],
      },
    });

    expect(getAgentDisplayPath(shared, 'global')).toBe('/home/alice/.agents/skills');
    expect(getAgentInstallPath(shared, 'global')).toBe('/home/alice/.agents/skills');
    expect(getAgentDisplayPath(privateAgent, 'global')).toBe('/home/alice/.private/skills');
    expect(getAgentInstallPath(privateAgent, 'global')).toBe('/home/alice/.private/skills');
    expect(getAgentDisplayPath(both, 'global')).toBe('/home/alice/.both/skills');
    expect(getAgentInstallPath(both, 'global')).toBe('/home/alice/.agents/skills');
    expect(canCreatePrivateCopy(both, 'global')).toBe(true);
  });

  it('normalizes local target paths for display on Windows', () => {
    expect(formatAgentTargetPath('C:\\Users\\cheng\\.gemini/antigravity/skills', 'win32'))
      .toBe('C:\\Users\\cheng\\.gemini\\antigravity\\skills');
    expect(formatAgentTargetPath('./.agents/skills/', 'win32'))
      .toBe('./.agents/skills');
  });

  it('keeps Unix-style target paths readable on non-Windows platforms', () => {
    expect(formatAgentTargetPath('C:\\Users\\cheng\\.gemini/antigravity/skills', 'posix'))
      .toBe('C:/Users/cheng/.gemini/antigravity/skills');
    expect(formatAgentTargetPath('./.agents/skills/', 'posix'))
      .toBe('./.agents/skills');
  });
});
