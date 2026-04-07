/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SkillsSection } from '../SkillsSection';
import type { InstalledSkill } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('../SkillCard', () => ({
  SkillCard: ({
    skill,
    updateStatus,
  }: {
    skill: InstalledSkill;
    updateStatus?: 'queued' | 'updating' | 'done' | 'failed';
  }) => (
    <div data-testid={`skill-card:${skill.scope}:${skill.name}`}>
      {updateStatus ?? 'idle'}
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
  ...overrides,
});

describe('SkillsSection', () => {
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
        onCancelUpdateAll={vi.fn()}
        onDelete={vi.fn()}
        onAdd={vi.fn()}
      />
    );

    expect(screen.getByTestId('skill-card:global:toolkit').textContent).toBe('updating');
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
        onCancelUpdateAll={vi.fn()}
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
        onCancelUpdateAll={vi.fn()}
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
        onCancelUpdateAll={vi.fn()}
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
        onCancelUpdateAll={vi.fn()}
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
});
