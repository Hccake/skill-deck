/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ComponentProps } from 'react';
import type { AgentType } from '@/bindings';
import { useSkillsDataStore } from '@/stores/skills-data';
import { UpdatePlanDialog } from '../UpdatePlanDialog';
import type { UpdatePlan } from '@/stores/skills-utils';
import { useMutationStore } from '@/stores/mutation';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const plan: UpdatePlan = {
  scope: 'global',
  total: 4,
  updatableCount: 4,
  repairableCount: 1,
  skippedCount: 1,
  groups: [
    {
      id: 'https://github.com/owner/repo::main',
      source: 'owner/repo',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: 'main',
      skillNames: ['toolkit', 'reviewer'],
      agents: ['claude-code', 'cursor', 'codex', 'kiro-cli', 'opencode'],
      skillRows: [
        { name: 'toolkit', agents: ['claude-code', 'cursor'] },
        { name: 'reviewer', agents: ['claude-code', 'cursor', 'codex', 'kiro-cli', 'opencode'] },
      ],
    },
    {
      id: 'https://github.com/owner/repo::beta',
      source: 'owner/repo',
      sourceUrl: 'https://github.com/owner/repo',
      gitRef: 'beta',
      skillNames: ['experimental'],
      agents: ['codex'],
      skillRows: [
        { name: 'experimental', agents: ['codex'] },
      ],
    },
  ] as UpdatePlan['groups'],
  repairable: [{ name: 'legacy', reason: 'missing-skill-path' }],
  skipped: [{ name: 'local-only', reason: 'local-source' }],
};

describe('UpdatePlanDialog', () => {
  beforeEach(() => {
    useSkillsDataStore.setState({
      lastUpdateResults: null,
      lastFailedUpdateNames: [],
    });
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('disables confirmation while keeping cancel available during another mutation', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        statusText: 'Installing',
        cancelable: true,
      },
    });

    render(
      <UpdatePlanDialog
        open
        plan={plan}
        onOpenChange={vi.fn()}
        onConfirm={vi.fn(async () => undefined)}
      />
    );

    expect((screen.getByRole('button', { name: 'skills.updatePlan.confirm' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'common.cancel' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows update scope as grouped skill rows without maintenance items', () => {
    const agentDisplayNames = new Map<AgentType, string>([
      ['claude-code', 'Claude Code'],
      ['cursor', 'Cursor'],
      ['codex', 'Codex'],
      ['kiro-cli', 'Kiro CLI'],
      ['opencode', 'OpenCode'],
    ]);

    render(
      <UpdatePlanDialog
        open
        plan={plan}
        agentDisplayNames={agentDisplayNames}
        onOpenChange={vi.fn()}
        onConfirm={vi.fn(async () => undefined)}
      />
    );

    expect(screen.getByText('skills.updatePlan.readyTitle')).toBeTruthy();
    expect(screen.getByText('skills.updatePlan.readyDescription')).toBeTruthy();
    expect(screen.queryByText('skills.updatePlan.groupNotice')).toBeNull();
    expect(screen.getByText('skills.updatePlan.confirm')).toBeTruthy();
    expect(screen.queryByRole('checkbox')).toBeNull();

    expect(screen.getAllByText('owner/repo')).toHaveLength(2);
    expect(screen.getAllByText('skills.refBadge')).toHaveLength(2);
    expect(screen.getByText('toolkit')).toBeTruthy();
    expect(screen.getByText('reviewer')).toBeTruthy();
    expect(screen.getByText('experimental')).toBeTruthy();
    expect(screen.getAllByText('Claude Code')[0]?.className).not.toContain('bg-primary/10');
    expect(screen.getAllByText('Cursor')[0]?.className).not.toContain('text-primary');
    expect(screen.getAllByText('Codex').length).toBeGreaterThan(0);
    expect(screen.getByText('+2')).toBeTruthy();

    expect(screen.queryByText('legacy')).toBeNull();
    expect(screen.queryByText('local-only')).toBeNull();
  });

  it('shows upstream-deleted skills as maintenance items', () => {
    render(
      <UpdatePlanDialog
        open
        plan={{
          scope: 'project',
          projectPath: '/repo',
          total: 1,
          updatableCount: 0,
          repairableCount: 0,
          skippedCount: 0,
          deletedUpstreamCount: 1,
          groups: [],
          repairable: [],
          skipped: [],
          deletedUpstream: [{
            name: 'demo',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
            reason: 'deleted-upstream',
            repairSource: 'https://github.com/owner/repo#main',
          }],
        }}
        agentDisplayNames={new Map()}
        onOpenChange={vi.fn()}
        onConfirm={vi.fn(async () => undefined)}
      />
    );

    expect(screen.getByText('skills.updatePlan.deletedUpstreamTitle')).toBeTruthy();
    expect(screen.getByText('skills.updatePlan.deletedUpstreamDescription')).toBeTruthy();
    expect(screen.getByText('demo')).toBeTruthy();
    expect(screen.getByText('https://github.com/owner/repo')).toBeTruthy();
  });

  it('shows target agents and completed result details with install modes and retry action', async () => {
    const onRetryFailed = vi.fn(async () => undefined);
    const props = {
      open: true,
      plan,
      onOpenChange: vi.fn(),
      onConfirm: vi.fn(async () => {
        useSkillsDataStore.setState({
          lastUpdateResults: [{
            name: 'toolkit',
            status: 'partial',
            reason: 'agent-failed',
            agentResults: [
              { agent: 'claude-code', status: 'success', mode: 'symlink' },
              { agent: 'cursor', status: 'failed', mode: 'copy', error: 'permission denied' },
            ],
          } as never],
          lastFailedUpdateNames: ['toolkit'],
        });
      }),
      onRetryFailed,
    } as ComponentProps<typeof UpdatePlanDialog> & { onRetryFailed: () => Promise<void> };

    render(<UpdatePlanDialog {...props} />);

    expect(screen.getAllByText('claude-code').length).toBeGreaterThan(0);
    expect(screen.getAllByText('cursor').length).toBeGreaterThan(0);

    fireEvent.click(screen.getByText('skills.updatePlan.confirm'));

    await waitFor(() => {
      expect(screen.getByText('skills.updatePlan.resultTitle')).toBeTruthy();
    });

    expect(screen.getByText('copy')).toBeTruthy();
    expect(screen.getByText('permission denied')).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByTitle('skills.updatePlan.retryFailed'));
    });

    expect(onRetryFailed).toHaveBeenCalledTimes(1);
  });
});
