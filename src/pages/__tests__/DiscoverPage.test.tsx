/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { DiscoverPage } from '../DiscoverPage';
import type { ContextRef } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const mocks = vi.hoisted(() => ({
  skillsDataState: {
    globalSkills: [] as Array<{ name: string; source?: string | null }>,
    projectSkills: [] as Array<{ name: string; source?: string | null }>,
    allProjectsSkills: new Map<string, Array<{ name: string; source?: string | null }>>(),
    fetchAllProjectsSkills: vi.fn(),
  },
  skillDialogState: {
    openAddWithPrefill: vi.fn(),
  },
  contextState: {
    projects: [] as string[],
    projectsLoaded: true,
    selectedContextRef: {
      environment: { kind: 'host' as const },
      scope: { scope: 'global' as const },
    } as ContextRef,
    hasExplicitContext: false,
  },
  environmentState: {
    projectsByEnvironment: {} as Record<string, unknown[]>,
    projectsLoaded: {} as Record<string, boolean>,
  },
  resizable: {
    groups: [] as Array<Record<string, unknown>>,
    panels: [] as Array<Record<string, unknown>>,
  },
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: typeof mocks.skillsDataState) => unknown) => selector(mocks.skillsDataState),
}));

vi.mock('@/stores/skill-dialog', () => ({
  useSkillDialogStore: (selector: (state: typeof mocks.skillDialogState) => unknown) => selector(mocks.skillDialogState),
}));

vi.mock('@/stores/context', () => ({
  useContextStore: (selector: (state: typeof mocks.contextState) => unknown) => selector(mocks.contextState),
}));

vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: { kind: string; distro_name?: string }) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name}`
  ),
  useEnvironmentStore: (selector: (state: typeof mocks.environmentState) => unknown) => selector(mocks.environmentState),
}));

vi.mock('@/components/skills/discover/DiscoverListPanel', () => ({
  DiscoverListPanel: () => <div>discover-list-panel</div>,
}));

vi.mock('@/components/skills/discover/DiscoverDetailPanel', () => ({
  DiscoverDetailPanel: () => <div>discover-detail-panel</div>,
}));

vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.groups.push(props);
    return <div>{children}</div>;
  },
  ResizablePanel: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.panels.push(props);
    return <div>{children}</div>;
  },
  ResizableHandle: () => <div>handle</div>,
}));

describe('DiscoverPage', () => {
  beforeEach(() => {
    mocks.skillsDataState.globalSkills = [];
    mocks.skillsDataState.projectSkills = [];
    mocks.skillsDataState.allProjectsSkills = new Map();
    mocks.skillsDataState.fetchAllProjectsSkills.mockReset();
    mocks.skillDialogState.openAddWithPrefill.mockReset();
    mocks.contextState.projects = [];
    mocks.contextState.projectsLoaded = true;
    mocks.contextState.selectedContextRef = {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    };
    mocks.contextState.hasExplicitContext = false;
    mocks.environmentState.projectsByEnvironment = {};
    mocks.environmentState.projectsLoaded = {};
    mocks.resizable.groups.length = 0;
    mocks.resizable.panels.length = 0;
  });

  it('uses explicit percentage sizing for discover panels', () => {
    render(<DiscoverPage />);

    expect(screen.getByText('discover-list-panel')).toBeTruthy();

    expect(mocks.resizable.groups[0]).toMatchObject({
      id: 'discover-page-layout-fixed',
      orientation: 'horizontal',
    });

    expect(mocks.resizable.panels[0]).toMatchObject({
      id: 'discover-list-fixed',
      defaultSize: '30%',
      minSize: '20%',
      maxSize: '50%',
    });

    expect(mocks.resizable.panels[1]).toMatchObject({
      id: 'discover-detail-fixed',
      defaultSize: '70%',
      minSize: '30%',
    });
  });

  it('loads installed project locations when the explicit environment is ready', () => {
    mocks.contextState.projectsLoaded = false;
    mocks.contextState.hasExplicitContext = true;
    mocks.contextState.selectedContextRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    mocks.environmentState.projectsByEnvironment = {
      'wsl:Ubuntu': [{ id: 'project-1', nativePath: '/home/me/app' }],
    };
    mocks.environmentState.projectsLoaded = { 'wsl:Ubuntu': true };

    render(<DiscoverPage />);

    expect(mocks.skillsDataState.fetchAllProjectsSkills).toHaveBeenCalledTimes(1);
  });
});
