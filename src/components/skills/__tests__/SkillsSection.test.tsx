/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SkillsSection } from '../SkillsSection';
import type { InstalledSkill } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('../SkillCard', () => ({
  SkillCard: ({
    skill,
    updateStatus,
    onRepairSource,
  }: {
    skill: InstalledSkill;
    updateStatus?: 'updating' | 'done' | 'failed';
    onRepairSource?: (skill: InstalledSkill) => void;
  }) => (
    <div data-testid={`skill-card:${skill.scope}:${skill.name}`}>
      <span data-testid={`status:${skill.scope}:${skill.name}`}>{updateStatus ?? 'idle'}</span>
      <button type="button" data-testid={`repair:${skill.scope}:${skill.name}`} onClick={() => onRepairSource?.(skill)}>
        repair
      </button>
    </div>
  ),
}));

const makeSkill = (
  scope: 'global' | 'project',
  overrides: Partial<InstalledSkill> = {},
): InstalledSkill => ({
  name: 'toolkit',
  description: '',
  path: `/skills/${scope}/toolkit`,
  canonicalPath: `/canonical/${scope}/toolkit`,
  scope,
  agents: [],
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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

  it('hides the check-updates action when no skills in the section can be checked', () => {
    render(
      <SkillsSection
        title="Global"
        skills={[
          {
            ...makeSkill('global', { hasUpdate: false, canCheckForUpdates: false }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' },
        ]}
        scope="global"
        updatingSkills={new Map()}
        isCheckingUpdates={false}
        onSkillClick={vi.fn()}
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
        onDelete={vi.fn()}
        onRepairSource={onRepairSource}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByTestId('repair:project:toolkit'));

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ scope: 'project', name: 'toolkit' }));
  });

  it('opens an update plan before running update all', async () => {
    const onUpdateAll = vi.fn(async () => undefined);

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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={onUpdateAll}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    fireEvent.click(screen.getByText('skills.updateAll'));

    expect(screen.getByText('skills.updatePlan.readyTitle')).toBeTruthy();
    expect(onUpdateAll).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText('skills.updatePlan.confirm'));

    await waitFor(() => {
      expect(onUpdateAll).toHaveBeenCalledWith('global');
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
            updateStatus: 'cannot-check',
            source: 'owner/repo',
            sourceUrl: 'https://github.com/owner/repo',
            gitRef: 'main',
          } as Partial<InstalledSkill>),
        ]}
        scope="project"
        projectPath="D:\\Code\\project-a"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
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
            updateStatus: 'cannot-check',
            updateReason: 'missing-skill-path',
          } as Partial<InstalledSkill>),
        ]}
        scope="global"
        updatingSkills={new Map()}
        onSkillClick={vi.fn()}
        onUpdate={vi.fn(async () => undefined)}
        onUpdateAll={vi.fn(async () => undefined)}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByText('skills.updateAll')).toBeTruthy();
    expect(screen.getByText('skills.uncheckableUpdateCount')).toBeTruthy();
    expect(screen.queryByText('skills.maintenanceNotice')).toBeNull();
  });
});
