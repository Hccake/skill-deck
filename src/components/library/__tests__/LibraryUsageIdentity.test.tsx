/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { LibraryUsageIdentity } from '../LibraryUsageIdentity';
import type { LibraryUsage, RegisteredProject } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const project = (
  nativePath: string,
  displayName: string | null = null,
): RegisteredProject => ({
  id: 'project-1',
  nativePath,
  displayName,
  order: null,
  suppressCrossStorageWarning: false,
});

const usage = (binding: RegisteredProject | null): LibraryUsage => ({
  context: {
    environment: { kind: 'native' },
    scope: binding
      ? { scope: 'project', project_id: binding.id }
      : { scope: 'global' },
  },
  project: binding,
  state: 'confirmed',
});

describe('LibraryUsageIdentity', () => {
  it('reuses the configured project name and keeps its path secondary', () => {
    render(<LibraryUsageIdentity usage={usage(project('/work/skill-deck', '  Skill Deck  '))} />);

    expect(screen.getByText('Skill Deck')).toBeTruthy();
    expect(screen.getByText('/work/skill-deck')).toBeTruthy();
  });

  it('falls back to the shared cross-platform basename rule', () => {
    render(<LibraryUsageIdentity usage={usage(project('C:\\Code\\skill-deck'))} />);

    expect(screen.getByText('skill-deck')).toBeTruthy();
    expect(screen.getByText('C:\\Code\\skill-deck')).toBeTruthy();
  });

  it('presents the global Skill location without inventing a path', () => {
    render(<LibraryUsageIdentity usage={usage(null)} />);

    expect(screen.getByText('libraries.usage.globalLocation')).toBeTruthy();
    expect(screen.queryByTestId('library-usage-path')).toBeNull();
  });
});
