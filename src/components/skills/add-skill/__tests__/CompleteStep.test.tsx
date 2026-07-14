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
      if (key === 'addSkill.complete.defaultAvailable') {
        return 'Ready to use';
      }
      if (key === 'addSkill.complete.privateAdapted') {
        return 'Separate setup';
      }
      if (key === 'addSkill.complete.privateCopies') {
        return 'Kept separately';
      }
      if (key === 'addSkill.complete.skippedCategory') {
        return 'Not written';
      }
      if (key === 'addSkill.complete.failedCategory') {
        return 'Failed';
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
      if (key === 'addSkill.complete.skipped') {
        return `Not written: ${options?.agents}`;
      }
      if (key === 'addSkill.error.unknown') {
        return 'Unknown error';
      }
      return key;
    },
  }),
}));

vi.mock('@/utils/cross-storage-guidance', () => ({
  getCrossStorageFailureGuidance: () => 'crossStorage.failureGuidance',
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
    skipped: false,
    error: null,
    ...partial,
  };
}

function makeState(installResults: InstallResults): WizardState {
  return {
    step: 'complete',
    entryPoint: 'skills-panel',
    scope: 'global',
    context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
    source: 'test/repo',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    riskPolicy: null,
    riskAcknowledged: false,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: [],
    privateCopyAgents: [],
    allAgents: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    privateCopyAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: true,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults,
    retrySkillName: undefined,
    retryAgents: undefined,
    retryAgentTargets: undefined,
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

  it('shows storage-owner guidance once for failed project installation results', () => {
    const installResults: InstallResults = {
      successful: [],
      failed: [makeInstallResult({
        success: false,
        error: 'permission denied',
      })],
      symlinkFallbackAgents: [],
    };

    render(
      <CompleteStep
        state={{
          ...makeState(installResults),
          scope: 'project',
          context: {
            environment: { kind: 'wsl', distro_name: 'Ubuntu' },
            scope: { scope: 'project', project_id: 'project-1' },
          },
        }}
        onDone={() => undefined}
      />
    );

    expect(screen.getByText('crossStorage.failureGuidance')).toBeDefined();
  });

  it('does not count skipped project agents as installed coverage', () => {
    const installResults: InstallResults = {
      successful: [
        makeInstallResult({ skillName: 'skill-a', agent: 'claude-code' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'windsurf', skipped: true }),
      ],
      failed: [],
      symlinkFallbackAgents: [],
    };

    render(<CompleteStep state={makeState(installResults)} onDone={() => undefined} />);

    expect(screen.getByText('1/2 agents')).toBeDefined();
    expect(screen.getByText('Not written: windsurf')).toBeDefined();
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
    expect(retrySpy).toHaveBeenCalledWith('skill-a', ['windsurf'], []);
  });

  it('retries failed Eve concrete targets without collapsing them to the root agent', () => {
    const retrySpy = vi.fn();
    const installResults: InstallResults = {
      successful: [],
      failed: [
        makeInstallResult({
          skillName: 'skill-a',
          agent: 'eve',
          targetId: 'eve:research',
          subagent: 'research',
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
    expect(retrySpy).toHaveBeenCalledWith(
      'skill-a',
      [],
      [{ agent: 'eve', subagent: 'research' }],
    );
  });

  it('summarizes ready to use, separate setup, kept-separately, skipped, and failed categories', () => {
    const installResults: InstallResults = {
      successful: [
        makeInstallResult({ skillName: 'skill-a', agent: 'codex', category: 'default-available' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'cursor', category: 'private-adapted' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'firebender', category: 'private-copy' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'missing-agent', skipped: true, category: 'skipped' }),
      ],
      failed: [
        makeInstallResult({
          skillName: 'skill-a',
          agent: 'windsurf',
          success: false,
          category: 'failed',
          error: 'permission denied',
        }),
      ],
      symlinkFallbackAgents: [],
    };

    render(<CompleteStep state={makeState(installResults)} onDone={() => undefined} />);

    expect(screen.getByText('3/5 agents')).toBeDefined();
    expect(screen.getByText('Ready to use · 1')).toBeDefined();
    expect(screen.getByText('Separate setup · 1')).toBeDefined();
    expect(screen.getByText('Kept separately · 1')).toBeDefined();
    expect(screen.getByText('Not written · 1')).toBeDefined();
    expect(screen.getByText('Failed · 1')).toBeDefined();
  });

  it('uses result summary fields to count default-available coverage', () => {
    const installResults: InstallResults = {
      successful: [
        makeInstallResult({ skillName: 'skill-a', agent: '__canonical__', category: 'default-available' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'cursor', category: 'private-adapted' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'firebender', category: 'private-copy' }),
      ],
      failed: [],
      symlinkFallbackAgents: [],
      defaultAvailableAgents: ['codex', 'opencode'],
      privateAdaptedAgents: ['cursor'],
      privateCopyAgents: ['firebender'],
    };

    render(<CompleteStep state={makeState(installResults)} onDone={() => undefined} />);

    expect(screen.getByText('4/4 agents')).toBeDefined();
    expect(screen.getByText('Ready to use · 2')).toBeDefined();
    expect(screen.getByText('Separate setup · 1')).toBeDefined();
    expect(screen.getByText('Kept separately · 1')).toBeDefined();
  });

  it('does not double count ready-to-use agents that are also kept separately', () => {
    const installResults: InstallResults = {
      successful: [
        makeInstallResult({ skillName: 'skill-a', agent: '__canonical__', category: 'default-available' }),
        makeInstallResult({ skillName: 'skill-a', agent: 'firebender', category: 'private-copy' }),
      ],
      failed: [],
      symlinkFallbackAgents: [],
      defaultAvailableAgents: ['codex', 'firebender'],
      privateAdaptedAgents: [],
      privateCopyAgents: ['firebender'],
    };

    render(<CompleteStep state={makeState(installResults)} onDone={() => undefined} />);

    expect(screen.getByText('2/2 agents')).toBeDefined();
    expect(screen.getByText('Ready to use · 1')).toBeDefined();
    expect(screen.getByText('Kept separately · 1')).toBeDefined();
  });

  it('does not count default-available summary without a canonical success result', () => {
    const installResults: InstallResults = {
      successful: [],
      failed: [
        makeInstallResult({
          skillName: 'skill-a',
          agent: 'firebender',
          success: false,
          category: 'failed',
          error: 'copy failed',
        }),
      ],
      symlinkFallbackAgents: [],
      defaultAvailableAgents: ['codex'],
      privateAdaptedAgents: [],
      privateCopyAgents: ['firebender'],
    };

    render(<CompleteStep state={makeState(installResults)} onDone={() => undefined} />);

    expect(screen.getByText('0/1 agents')).toBeDefined();
    expect(screen.queryByText(/Ready to use/)).toBeNull();
    expect(screen.getByText('Failed · 1')).toBeDefined();
  });
});
