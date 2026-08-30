/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { LibraryApplicationStrip } from '../LibraryApplicationStrip';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

describe('LibraryApplicationStrip', () => {
  it('renders applied libraries in priority order with their Skill counts', () => {
    render(<LibraryApplicationStrip application={{
      orderedLibraries: [
        { id: 'first', name: 'First', skillCount: 3 },
        { id: 'second', name: 'Second', skillCount: 1 },
      ],
      selectedAgentIds: [],
      pending: false,
    }} />);

    const summary = screen.getByTestId('applied-libraries-summary');
    const libraries = within(summary).getAllByTestId('library-summary-item');
    expect(libraries.map((library) => library.getAttribute('title'))).toEqual([
      'First',
      'Second',
    ]);
    expect(within(libraries[0]).getByText('libraries.skillCount:{"count":3}')).toBeTruthy();
    expect(within(libraries[1]).getByText('libraries.skillCount:{"count":1}')).toBeTruthy();
    expect(within(summary).queryByRole('button')).toBeNull();
    expect(within(summary).queryByRole('link')).toBeNull();
  });

  it('shows a pending adjustment even when no Library is currently applied', () => {
    render(<LibraryApplicationStrip application={{
      orderedLibraries: [],
      selectedAgentIds: [],
      pending: true,
    }} />);

    expect(screen.getByRole('status').textContent).toBe('libraries.pending');
  });

  it('does not reserve space without applied or pending Libraries', () => {
    render(<LibraryApplicationStrip application={{
      orderedLibraries: [],
      selectedAgentIds: [],
      pending: false,
    }} />);

    expect(screen.queryByTestId('applied-libraries-summary')).toBeNull();
  });
});
