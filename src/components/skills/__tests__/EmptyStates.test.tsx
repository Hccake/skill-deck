/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { GlobalEmptyState, ProjectEmptyState } from '../EmptyStates';
import { useMutationStore } from '@/stores/mutation';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe('skill empty states', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('disables every empty-state add action during another mutation', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: { environment: { kind: 'host' }, scope: { scope: 'global' } },
        statusText: 'Updating',
        cancelable: true,
      },
    });

    const { rerender } = render(<GlobalEmptyState onAdd={vi.fn()} />);
    expect((screen.getByRole('button', { name: 'skills.add' }) as HTMLButtonElement).disabled).toBe(true);

    rerender(<ProjectEmptyState onAdd={vi.fn()} />);
    expect((screen.getByRole('button', { name: 'skills.add' }) as HTMLButtonElement).disabled).toBe(true);
  });
});
