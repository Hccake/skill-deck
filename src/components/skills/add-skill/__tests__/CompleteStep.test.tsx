/* @vitest-environment jsdom */

import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { InstallResult, InstallResults } from '@/bindings';
import type { WizardState } from '../types';
import { CompleteStep } from '../CompleteStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === 'addSkill.complete.agentCoverage') {
        return `${options?.success}/${options?.total} agents`;
      }
      if (key === 'addSkill.complete.showFailures') {
        return `Failures (${options?.count})`;
      }
      if (key === 'addSkill.complete.hideFailures') {
        return 'Hide failures';
      }
      if (key === 'addSkill.actions.retrySkill') {
        return 'Retry Skill';
      }
      if (key === 'addSkill.complete.partial') {
        return 'Installation completed with errors';
      }
      if (key === 'addSkill.actions.done') {
        return 'Done';
      }
      if (key === 'addSkill.actions.retry') {
        return 'Retry';
      }
      if (key === 'addSkill.complete.successCount') {
        return `Successful: ${options?.count}`;
      }
      if (key === 'addSkill.complete.failedCount') {
        return `Failed: ${options?.count}`;
      }
      if (key === 'addSkill.error.unknown') {
        return 'Unknown error';
      }
      return key;
    },
  }),
}));

function makeInstallResult(partial?: Partial<InstallResult>): InstallResult {
  return {
    skillName: 'skill-a',
    agent: 'cursor',
    success: true,
    path: '/tmp/skill-a',
    canonicalPath: '/tmp/.agents/skill-a',
    mode: 'symlink',
    symlinkFailed: false,
    error: null,
    ...partial,
  };
}

function makeState(installResults: InstallResults): WizardState {
  return {
    step: 'complete',
    entryPoint: 'skills-panel',
    scope: 'global',
    source: 'test/repo',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: [],
    allAgents: [],
    agentsCollapsed: false,
    mode: 'symlink',
    otherAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: true,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults,
    retrySkillName: undefined,
    retryAgents: undefined,
  };
}

describe('CompleteStep', () => {
  it('shows skill-level coverage and keeps failed details collapsed by default', () => {
    const installResults: InstallResults = {
      successful: [
        makeInstallResult({ skillName: 'skill-a', agent: 'cursor' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'claude-code' }),
      ],
      failed: [
        makeInstallResult({
          skillName: 'skill-a',
          agent: 'windsurf',
          success: false,
          error: 'permission denied',
        }),
      ],
      symlinkFallbackAgents: [],
    };

    render(
      <CompleteStep
        state={makeState(installResults)}
        onDone={() => undefined}
        onRetry={() => undefined}
      />
    );

    expect(screen.getByText('2/3 agents')).toBeDefined();
    expect(screen.queryByText('permission denied')).toBeNull();
  });

  it('retries one failed skill with only failed agents', () => {
    const retrySpy = vi.fn();
    const installResults: InstallResults = {
      successful: [makeInstallResult({ skillName: 'skill-a', agent: 'cursor' })],
      failed: [
        makeInstallResult({
          skillName: 'skill-a',
          agent: 'windsurf',
          success: false,
          error: 'permission denied',
        }),
      ],
      symlinkFallbackAgents: [],
    };

    render(
      <CompleteStep
        state={makeState(installResults)}
        onDone={() => undefined}
        onRetry={() => undefined}
        onRetrySkill={retrySpy}
      />
    );

    fireEvent.click(screen.getByRole('button', { name: 'Retry Skill' }));
    expect(retrySpy).toHaveBeenCalledWith('skill-a', ['windsurf']);
  });
});
