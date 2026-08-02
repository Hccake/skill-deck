/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { GlobalEmptyState, ProjectEmptyState, SkillFilterEmptyState } from '../EmptyStates';
import { useMutationStore } from '@/stores/mutation';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) => ({
      'skills.filter.emptyAgent': `没有可供 ${values?.name} 使用的 Skill`,
      'skills.filter.emptySearch': `没有匹配“${values?.query}”的 Skill`,
      'skills.filter.emptyCombined': '没有符合当前条件的 Skill',
    }[key] ?? key),
  }),
}));

describe('skill empty states', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
  });

  it('disables add actions while the install wizard keeps the main window read-only', () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    render(<GlobalEmptyState onAdd={vi.fn()} />);

    expect((screen.getByRole('button', { name: 'skills.add' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('disables every empty-state add action during another mutation', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    const { rerender } = render(<GlobalEmptyState onAdd={vi.fn()} />);
    expect((screen.getByRole('button', { name: 'skills.add' }) as HTMLButtonElement).disabled).toBe(true);

    rerender(<ProjectEmptyState onAdd={vi.fn()} />);
    expect((screen.getByRole('button', { name: 'skills.add' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('uses one compact contextual sentence without duplicating the toolbar action', () => {
    render(
      <SkillFilterEmptyState
        agentName="Codex"
        searchQuery="writer"
      />,
    );

    expect(screen.getByRole('status')).toBeDefined();
    expect(screen.getByText('没有符合当前条件的 Skill')).toBeDefined();
    expect(screen.queryByRole('button')).toBeNull();
    expect(screen.queryByText('skills.filter.emptyDescription')).toBeNull();
  });

  it('names the selected Agent when it is the only active condition', () => {
    render(<SkillFilterEmptyState agentName="Codex" />);

    expect(screen.getByText('没有可供 Codex 使用的 Skill')).toBeDefined();
  });

  it('quotes the search term when it is the only active condition', () => {
    render(<SkillFilterEmptyState searchQuery="writer" />);

    expect(screen.getByText('没有匹配“writer”的 Skill')).toBeDefined();
  });
});
