/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { ContextSidebar } from '../ContextSidebar';
import zhCN from '@/i18n/locales/zh-CN.json';

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

  it('uses availability-scope language instead of treating global as a workspace', () => {
    expect(zhCN.context.global).toBe('全局');
    expect(zhCN.context.globalSubtitle).toBe('所有项目可用');
    expect(zhCN.context.sectionGlobal).toBe('全局');
    expect(zhCN.context.sectionProjects).toBe('项目');
  });

  it('omits the sidebar title and moves top spacing to the scroll area', () => {
    const { container, queryByRole } = render(<ContextSidebar />);
    const scrollArea = container.querySelector('[data-testid="context-sidebar-scroll"]');

    expect(queryByRole('heading', { level: 2, name: 'context.title' })).toBeNull();
    expect(scrollArea?.classList.contains('pt-5')).toBe(true);
  });

  it('uses usage-scope language for the first add-skill step', () => {
    expect(zhCN.addSkill.steps.scope).toBe('使用范围');
    expect(zhCN.addSkill.scopeSelect.title).toBe('使用范围');
    expect(zhCN.addSkill.scopeSelect.hint).toBe('选择此 Skill 的使用范围。');
    expect(zhCN.addSkill.scopeSelect.global).toBe('全局');
    expect(zhCN.addSkill.scopeSelect.globalHint).toBe('可在所有项目中使用 · {{path}}');
    expect(zhCN.addSkill.scopeSelect.localProjects).toBe('项目');
  });
});
