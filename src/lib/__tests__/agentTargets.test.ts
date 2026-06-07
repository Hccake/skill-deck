import { describe, expect, it } from 'vitest';
import type { AgentInfo } from '@/bindings';
import { makeAgentScopeTarget } from '@/test-utils';
import {
  filterAdditionalAgentIds,
  formatAgentTargetPath,
  getSharedSkillDirectory,
  groupAgentsByScopedTarget,
  migrateDefaultTargetAgents,
} from '../agentTargets';

function makeAgent(
  id: string,
  globalAutomatic: boolean,
  projectAutomatic: boolean,
  detected = true,
  targetOverrides: {
    global?: Partial<AgentInfo['targets']['global']>;
    project?: Partial<AgentInfo['targets']['project']>;
  } = {},
): AgentInfo {
  return {
    id: id as AgentInfo['id'],
    name: id,
    skillsDir: projectAutomatic ? '.agents/skills' : `.${id}/skills`,
    globalSkillsDir: globalAutomatic ? '~/.agents/skills' : `~/.${id}/skills`,
    detected,
    targets: {
      global: {
        ...makeAgentScopeTarget({
          automatic: globalAutomatic,
          path: globalAutomatic ? '~/.agents/skills' : `~/.${id}/skills`,
          ...targetOverrides.global,
        }),
      },
      project: makeAgentScopeTarget({
        automatic: projectAutomatic,
        path: projectAutomatic ? '.agents/skills' : `.${id}/skills`,
        sharedPath: './.agents/skills',
        ...targetOverrides.project,
      }),
    },
  };
}

describe('agent target helpers', () => {
  it('provides normalized shared skill directory display paths', () => {
    expect(getSharedSkillDirectory('global')).toBe('~/.agents/skills');
    expect(getSharedSkillDirectory('project')).toBe('./.agents/skills');
  });

  it('filters automatic agents from additional defaults per scope', () => {
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

  it('drops unsupported and unknown agent ids', () => {
    const unsupported = makeAgent('unsupported', false, false);
    unsupported.targets.global.supported = false;

    expect(filterAdditionalAgentIds(['unsupported', 'missing'], [unsupported], 'global'))
      .toEqual([]);
  });

  it('groups scoped targets by automatic, visible selectable, and hidden selectable state', () => {
    const automaticDetected = makeAgent('automatic-detected', true, true);
    const automaticUndetected = makeAgent('automatic-undetected', true, true, false);
    const selectableDetected = makeAgent('selectable-detected', false, false);
    const selectableSelected = makeAgent('selectable-selected', false, false, false);
    const selectableHidden = makeAgent('selectable-hidden', false, false, false);
    const unsupported = makeAgent('unsupported', false, false);
    unsupported.targets.global.supported = false;

    const groups = groupAgentsByScopedTarget(
      [
        automaticDetected,
        automaticUndetected,
        selectableDetected,
        selectableSelected,
        selectableHidden,
        unsupported,
      ],
      'global',
      new Set(['selectable-selected'])
    );

    expect(groups.detectedAutomatic.map((agent) => agent.id)).toEqual(['automatic-detected']);
    expect(groups.undetectedAutomatic.map((agent) => agent.id)).toEqual(['automatic-undetected']);
    expect(groups.detectedSelectableAgents.map((agent) => agent.id)).toEqual(['selectable-detected']);
    expect(groups.visibleSelectableAgents.map((agent) => agent.id)).toEqual([
      'selectable-detected',
      'selectable-selected',
    ]);
    expect(groups.hiddenSelectableAgents.map((agent) => agent.id)).toEqual(['selectable-hidden']);
    expect(groups.selectableCount).toBe(3);
  });

  it('groups default-available agents separately from private-required agents', () => {
    const defaultAgent = makeAgent('codex', true, true, true, {
      global: {
        availability: 'shared-compatible',
        defaultAvailable: true,
      },
    });
    const privateAgent = makeAgent('cursor', false, true, true, {
      global: {
        availability: 'private-required',
        defaultAvailable: false,
      },
    });

    const groups = groupAgentsByScopedTarget([defaultAgent, privateAgent], 'global');

    expect(groups.detectedDefaultAvailable.map((agent) => agent.id)).toEqual(['codex']);
    expect(groups.detectedPrivateRequired.map((agent) => agent.id)).toEqual(['cursor']);
  });

  it('filters default-available agents from default target preferences', () => {
    const agents = [
      makeAgent('firebender', true, true, true, {
        global: {
          availability: 'shared-compatible',
          defaultAvailable: true,
        },
      }),
      makeAgent('cursor', false, true, true, {
        global: {
          availability: 'private-required',
          defaultAvailable: false,
        },
      }),
    ];

    expect(filterAdditionalAgentIds(['firebender', 'cursor'], agents, 'global'))
      .toEqual(['cursor']);
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
