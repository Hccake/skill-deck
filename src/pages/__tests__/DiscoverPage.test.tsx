/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { DiscoverPage } from '../DiscoverPage';
import type { SkillLocationRef } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const mocks = vi.hoisted(() => ({
  skillsDataState: {
    snapshots: {} as Record<string, {
      skills: Array<{ name: string; source?: string | null }>;
      agents: unknown[];
      pathExists: boolean;
      loading: boolean;
      error: string | null;
      requestId: number;
    }>,
    refreshContext: vi.fn().mockResolvedValue(undefined),
  },
  skillDialogState: {
    openAddWithPrefill: vi.fn(),
  },
  workspaceContextState: {
    selectedContext: {
      environment: { kind: 'native' as const },
      scope: { scope: 'global' as const },
    } as SkillLocationRef,
  },
  projectState: {
    projectsByEnvironment: {} as Record<string, unknown[]>,
    refresh: vi.fn().mockResolvedValue([]),
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

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: typeof mocks.workspaceContextState) => unknown) =>
    selector(mocks.workspaceContextState),
}));

vi.mock('@/hooks/useProjectWorkspace', () => ({
  useProjectWorkspace: (environment: { kind: string; distro_name?: string }) => {
    const key = environment.kind === 'native' ? 'native' : `wsl:${environment.distro_name?.toLowerCase()}`;
    return {
      projects: mocks.projectState.projectsByEnvironment[key] ?? [],
      refresh: mocks.projectState.refresh,
    };
  },
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
    mocks.skillsDataState.snapshots = {
      'native/global': {
        skills: [], agents: [], pathExists: true, loading: false, error: null, requestId: 1,
      },
    };
    mocks.skillsDataState.refreshContext.mockReset();
    mocks.skillsDataState.refreshContext.mockResolvedValue(undefined);
    mocks.skillDialogState.openAddWithPrefill.mockReset();
    mocks.workspaceContextState.selectedContext = {
      environment: { kind: 'native' },
      scope: { scope: 'global' },
    };
    mocks.projectState.projectsByEnvironment = {};
    mocks.projectState.refresh.mockReset();
    mocks.projectState.refresh.mockResolvedValue([]);
    mocks.resizable.groups.length = 0;
    mocks.resizable.panels.length = 0;
  });

  it('refreshes context-keyed install locations for the committed environment', () => {
    mocks.workspaceContextState.selectedContext = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };
    mocks.projectState.projectsByEnvironment = {
      'wsl:ubuntu': [{
        binding: {
          id: 'project-1',
          nativePath: '/home/me/app',
          displayName: null,
          order: null,
          suppressCrossStorageWarning: false,
        },
        storage: {
          access: 'native',
          owner: { kind: 'wsl', distro_name: 'Ubuntu' },
        },
      }],
    };
    render(<DiscoverPage />);

    expect(mocks.skillsDataState.refreshContext).toHaveBeenCalledWith({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    });
    expect(mocks.skillsDataState.refreshContext).toHaveBeenCalledWith({
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    });
  });
});
