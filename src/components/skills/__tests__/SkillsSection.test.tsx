/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SkillsSection } from '../SkillsSection';
import type { InstalledSkill } from '@/bindings';
import type { SkillListItem } from '@/stores/skills-utils';
import { useMutationStore } from '@/stores/mutation';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en' },
  }),
}));

vi.mock('../SkillCard', () => ({
  SkillCard: ({
    skill,
    updateStatus,
    onUpdate,
    onRepairSource,
  }: {
    skill: InstalledSkill;
    updateStatus?: 'acquiring' | 'validating' | 'updating' | 'done' | 'failed';
    onUpdate?: (skillName: string) => void;
    onRepairSource?: (skill: InstalledSkill) => void;
  }) => (
    <div data-testid={`skill-card:${skill.scope}:${skill.name}`}>
      <span data-testid={`status:${skill.scope}:${skill.name}`}>{updateStatus ?? 'idle'}</span>
      <button type="button" data-testid={`update:${skill.scope}:${skill.name}`} onClick={() => onUpdate?.(skill.name)}>
        update
      </button>
      <button type="button" data-testid={`repair:${skill.scope}:${skill.name}`} onClick={() => onRepairSource?.(skill)}>
        repair
      </button>
    </div>
  ),
}));

const makeSkill = (
  scope: 'global' | 'project',
  overrides: Partial<SkillListItem> = {},
): SkillListItem => ({
  name: 'toolkit',
  description: '',
  path: `/skills/${scope}/toolkit`,
  canonicalPath: `/canonical/${scope}/toolkit`,
  scope,
  agents: [],
  associatedAgents: [],
  hasUpdate: true,
  canCheckForUpdates: true,
  ...overrides,
});

describe('SkillsSection', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('disables write actions but keeps update checks available during another mutation', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'install',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    expect((screen.getByRole('button', { name: 'skills.updateAll' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'skills.add' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'skills.checkUpdates' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('reads update state using the full skill identity key', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map([['global:toolkit', 'updating']])}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByTestId('status:global:toolkit').textContent).toBe('updating');
  });

  it('does not show a completed check state after external polling finishes', async () => {
    const { rerender } = render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    rerender(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    rerender(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global')]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    await waitFor(() => {
      expect(screen.queryByText('skills.updateDone')).toBeNull();
    });
  });

  it('shows a completed check state only after an explicit successful check action', async () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', { hasUpdate: false })]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    fireEvent.click(screen.getByText('skills.checkUpdates'));

    await waitFor(() => {
      expect(screen.getByText('skills.updateDone')).toBeTruthy();
    });
  });

  it('shows an inaccessible project as a neutral empty state without actions', () => {
    render(
      <SkillsSection
        title="Project"
        skills={[]}
        scope="project"
        pathExists={false}
        projectPath="D:\\Code\\project-a"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    const unavailable = screen.getByRole('status', {
      name: 'skills.projectUnavailableTitle',
    });

    expect(screen.getByText('skills.projectUnavailableDescription')).toBeTruthy();
    expect(screen.queryByText('skills.projectNotFound')).toBeNull();
    expect(screen.queryByText('skills.upToDate')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
    expect(unavailable.className).toContain('border-dashed');
    expect(unavailable.className).not.toContain('border-l-');
    expect(unavailable.className).not.toContain('warning');
    expect(unavailable.className).not.toContain('amber');
  });

  it('does not report the filtered result as up to date', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[]}
        scope="global"
        filterActive
        updatingSkills={new Map()}
        emptyState={<div>filtered-empty</div>}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />,
    );

    expect(screen.getByText('filtered-empty')).toBeDefined();
    expect(screen.queryByText('skills.upToDate')).toBeNull();
  });

  it('summarizes failed Skill checks without exposing source diagnostics', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            name: 'toolkit',
            hasUpdate: false,
            canCheckForUpdates: true,
            updateStatus: 'cannotCheck',
            updateReason: 'rate-limited',
          }),
          makeSkill('global', {
            name: 'writer',
            hasUpdate: false,
            canCheckForUpdates: true,
            updateStatus: 'cannotCheck',
            updateReason: 'network-error',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    expect(screen.getByText('skills.updateCheckFailureCount')).toBeTruthy();
    expect(screen.queryByText('skills.uncheckableUpdateCount')).toBeNull();
    expect(screen.queryByText('skills.upToDate')).toBeNull();
    expect(screen.queryByText('github.com/owner/stale')).toBeNull();
    expect(screen.queryByText('github.com/owner/cooling-down')).toBeNull();
    expect(screen.queryByLabelText('skills.updateEvidence.title')).toBeNull();
  });

  it('keeps the update check action available when the backend reports a cooling state', async () => {
    const onCheckUpdates = vi.fn(async () => true);
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
          updateStatus: 'cannotCheck',
          updateReason: 'rate-limited',
        })]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={onCheckUpdates}
      />
    );

    const checkButton = screen.getByRole('button', { name: 'skills.checkUpdates' });
    expect(checkButton.getAttribute('aria-disabled')).toBeNull();
    fireEvent.click(checkButton);

    await waitFor(() => {
      expect(onCheckUpdates).toHaveBeenCalledTimes(1);
    });
  });

  it('hides the check-updates action when no skills in the section can be checked', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          {
            ...makeSkill('global', { hasUpdate: false, canCheckForUpdates: false }),
            updateStatus: 'cannotCheck',
          } as InstalledSkill & { updateStatus?: 'cannotCheck' },
        ]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    expect(screen.queryByText('skills.checkUpdates')).toBeNull();
  });

  it('hides the check-updates action when capability metadata is missing', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', { hasUpdate: false, canCheckForUpdates: undefined })]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    expect(screen.queryByText('skills.checkUpdates')).toBeNull();
  });

  it('passes repair source actions to skill cards', () => {
    const onRepairSource = vi.fn();

    render(
      <SkillsSection
        title="Project"
        skills={[makeSkill('project', { hasUpdate: false, updateReason: 'missing-skill-path' })]}
        scope="project"
        projectPath="D:\\Code\\project-a"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onRepairSource={onRepairSource}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId('repair:project:toolkit'));

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ scope: 'project', name: 'toolkit' }));
  });

  it('delegates update-all preview to the page-level update workflow owner', async () => {
    const onPrepareUpdate = vi.fn(async () => true);

    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={onPrepareUpdate}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByText('skills.updateAll'));

    await waitFor(() => {
      expect(onPrepareUpdate).toHaveBeenCalledWith(['toolkit'], true);
    });
    expect(screen.queryByText('skills.updatePlan.readyTitle')).toBeNull();
  });

  it('delegates a direct reinstall without a remote hash to the update workflow', async () => {
    const onPrepareUpdate = vi.fn(async () => true);

    render(
      <SkillsSection
        title="Global"
        skills={[makeSkill('global', {
          hasUpdate: false,
          canRunUpdate: true,
          canCheckForUpdates: false,
          updateStatus: 'cannotCheck',
          updateReason: 'missingRemoteHash',
        } as Partial<InstalledSkill>)]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={onPrepareUpdate}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId('update:global:toolkit'));

    await waitFor(() => {
      expect(onPrepareUpdate).toHaveBeenCalledWith(['toolkit'], false);
    });
  });

  it('renders update all as a unified secondary action when updates are available', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
        onCheckUpdates={vi.fn(async () => true)}
      />
    );

    const actions = screen.getByTestId('skills-section-actions');
    const secondaryActions = screen.getByTestId('skills-section-secondary-actions');
    const updateAll = screen.getByRole('button', { name: 'skills.updateAll' });
    const checkUpdates = screen.getByRole('button', { name: 'skills.checkUpdates' });

    expect(actions.contains(updateAll)).toBe(true);
    expect(secondaryActions.contains(updateAll)).toBe(true);
    expect(secondaryActions.contains(checkUpdates)).toBe(true);
    expect(actions.className).toContain('gap-2');
    expect(secondaryActions.className).toContain('gap-0.5');
    expect(updateAll.className).toContain('h-7');
    expect(updateAll.className).toContain('px-2');
    expect(updateAll.className).toContain('text-muted-foreground');
    expect(updateAll.getAttribute('data-variant')).toBe('ghost');
    expect(updateAll.className).not.toContain('h-auto');
    expect(updateAll.className).not.toContain('p-0');
    expect(updateAll.className).not.toContain('border-primary');
  });

  it('uses neutral summary styling for available update counts', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            canRunUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    const updateCount = screen.getByText('1 skills.update');

    expect(updateCount.className).not.toContain('text-warning');
    expect(updateCount.className).toContain('text-muted-foreground');
  });

  it('does not show update all when the section only has maintenance items', () => {
    render(
      <SkillsSection
        title="Project"
        skills={[
          makeSkill('project', {
            hasUpdate: false,
            canRunUpdate: false,
            canCheckForUpdates: false,
            updateReason: 'missing-skill-path',
            updateStatus: 'cannotCheck',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          } as Partial<InstalledSkill>),
        ]}
        scope="project"
        projectPath="D:\\Code\\project-a"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onRepairSource={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.queryByText('skills.updateAll')).toBeNull();
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBeTruthy();
    expect(screen.queryByText('skills.maintenanceNotice')).toBeNull();
  });

  it('shows update all only for directly updatable skills', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          makeSkill('global', {
            hasUpdate: true,
            canRunUpdate: true,
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          }),
          makeSkill('global', {
            name: 'legacy-toolkit',
            hasUpdate: false,
            canRunUpdate: false,
            canCheckForUpdates: false,
            updateStatus: 'cannotCheck',
            updateReason: 'missing-skill-path',
          } as Partial<InstalledSkill>),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onPrepareUpdate={vi.fn(async () => true)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByText('skills.updateAll')).toBeTruthy();
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBeTruthy();
    expect(screen.queryByText('skills.maintenanceNotice')).toBeNull();
  });
});
