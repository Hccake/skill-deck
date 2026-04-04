/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { ContextSidebar } from '../ContextSidebar';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
}));

vi.mock('@/stores/context', () => ({
  useContextStore: () => ({
    selectedContext: 'global',
    projects: [],
    projectsLoaded: true,
    loadProjects: vi.fn(),
    addProject: vi.fn(),
    removeProject: vi.fn(),
    selectContext: vi.fn(),
    toggleProjectContext: vi.fn(),
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  openInExplorer: vi.fn(),
}));

describe('ContextSidebar', () => {
  it('renders the semantic sidebar shell class instead of a fixed width utility', () => {
    const { container } = render(<ContextSidebar />);
    const aside = container.querySelector('aside');

    expect(aside?.classList.contains('skills-context-sidebar')).toBe(true);
    expect(aside?.classList.contains('w-64')).toBe(false);
  });
});