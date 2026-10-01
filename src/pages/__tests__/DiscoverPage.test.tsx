/* @vitest-environment jsdom */

import '@/test-utils';
import { TooltipProvider } from '@/components/ui/tooltip';

const render = (ui: React.ReactNode) => renderComponent(ui, { wrapper: TooltipProvider });
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render as renderComponent, screen, waitFor } from '@testing-library/react';
import { DiscoverPage } from '../DiscoverPage';
import { useLayoutStore } from '@/stores/layout';
import userEvent from '@testing-library/user-event';
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
  leaderboard: vi.fn(),
  detail: vi.fn(),
  resizable: {
    groups: [] as Array<Record<string, unknown>>,
    panels: [] as Array<Record<string, unknown>>,
    apply: vi.fn((layout: Record<string, number>) => layout),
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

vi.mock('@/lib/discover/api', () => ({
  getDiscoverLeaderboard: (...args: unknown[]) => mocks.leaderboard(...args),
  searchDiscoverSkills: (...args: unknown[]) => mocks.leaderboard(...args),
  getDiscoverSkillDetail: (...args: unknown[]) => mocks.detail(...args),
}));

vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.groups.push(props);
    const ref = props.groupRef as { current: unknown } | undefined;
    if (ref) ref.current = { getLayout: () => ({ 'discover-list-fixed': 30, 'discover-detail-fixed': 70 }), setLayout: mocks.resizable.apply };
    return <div>{children}</div>;
  },
  ResizablePanel: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.panels.push(props);
    return <div>{children}</div>;
  },
  ResizableHandle: ({ onKeyDownCapture }: React.ComponentProps<'div'>) => <div role="separator" tabIndex={0} onKeyDownCapture={onKeyDownCapture}>handle</div>,
}));

describe('DiscoverPage', () => {
  it('only saves explicit resizing and restores the desired width after a narrow window', () => {
    let width = 1280;
    let measure = () => {};
    const bounds = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(() => ({ width }) as DOMRect);
    vi.stubGlobal('ResizeObserver', class {
      constructor(callback: () => void) { measure = callback; }
      observe() {} unobserve() {} disconnect() {}
    });
    const view = render(<DiscoverPage />);
    try {
      const change = () => (mocks.resizable.groups.at(-1)!.onLayoutChanged as (layout: Record<string, number>) => void)({ 'discover-list-fixed': 40, 'discover-detail-fixed': 60 });
      act(change);
      expect(useLayoutStore.getState().discoverListWidth).toBeNull();
      fireEvent.keyDown(screen.getByRole('separator'), { key: 'ArrowRight' });
      act(change);
      const saved = useLayoutStore.getState().discoverListWidth!;
      expect(saved).toBeGreaterThan(500);
      act(() => { width = 720; measure(); });
      act(change);
      expect(useLayoutStore.getState().discoverListWidth).toBe(saved);
      const narrow = mocks.resizable.apply.mock.lastCall![0]['discover-list-fixed'];
      expect(narrow).toBeLessThan(50);
      act(() => { width = 1280; measure(); });
      const restored = mocks.resizable.apply.mock.lastCall![0]['discover-list-fixed'];
      expect(restored).toBeCloseTo(40, 0);
    } finally {
      view.unmount(); bounds.mockRestore(); vi.unstubAllGlobals();
    }
  });

  it('keeps selection separate from focus and closes back to the selected entry', async () => {
    const user = userEvent.setup();
    render(<DiscoverPage />);
    const alpha = await screen.findByRole('button', { name: /alpha/ });
    const beta = screen.getByRole('button', { name: /beta/ });
    act(() => alpha.focus());
    await user.keyboard('{Enter}{ArrowDown}');
    expect(screen.getByRole('heading', { name: 'alpha' })).toBeTruthy();
    expect(document.activeElement).toBe(beta);
    await user.keyboard('{Enter}');
    expect(screen.getByRole('heading', { name: 'beta' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: 'skills.discover.install' }));
    expect(mocks.skillDialogState.openAddWithPrefill).toHaveBeenCalledWith(
      { source: 'owner/repo', skillName: 'beta' }, mocks.workspaceContextState.selectedContext,
    );
    await user.click(screen.getByRole('button', { name: 'common.close' }));
    expect(document.activeElement).toBe(beta);
    expect(screen.queryByRole('heading', { name: 'beta' })).toBeNull();
    expect(screen.getByRole('heading', { name: 'skills.discover.emptyTitle' })).toBeTruthy();
  });

  it('lets search consume Escape before closing and does not consume composition or nested controls', async () => {
    const user = userEvent.setup();
    render(<DiscoverPage />);
    await user.click(await screen.findByRole('button', { name: /alpha/ }));
    const search = screen.getByRole('searchbox');
    await user.type(search, 'missing');
    await user.keyboard('{Escape}');
    expect((search as HTMLInputElement).value).toBe('');
    expect(screen.getByRole('heading', { name: 'alpha' })).toBeTruthy();
    fireEvent.keyDown(search, { key: 'Escape', isComposing: true });
    expect(screen.getByRole('heading', { name: 'alpha' })).toBeTruthy();
    const close = screen.getByRole('button', { name: 'common.close' });
    close.addEventListener('keydown', event => event.preventDefault(), { once: true });
    fireEvent.keyDown(close, { key: 'Escape' });
    expect(screen.getByRole('heading', { name: 'alpha' })).toBeTruthy();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('heading', { name: 'alpha' })).toBeNull();
    expect(document.activeElement).toBe(search);
  });

  it('keeps the same detail mounted but gives a newly selected skill a fresh reading viewport', async () => {
    const user = userEvent.setup();
    render(<DiscoverPage />);
    const alpha = await screen.findByRole('button', { name: /alpha/ });
    await user.click(alpha);
    await waitFor(() => expect(screen.queryByTestId('discover-detail-skeleton')).toBeNull());
    const viewport = document.querySelector<HTMLElement>('[data-radix-scroll-area-viewport]')!;
    viewport.scrollTop = 180;
    await user.click(alpha);
    expect(document.querySelector('[data-radix-scroll-area-viewport]')).toBe(viewport);
    expect(viewport.scrollTop).toBe(180);
    await user.click(screen.getByRole('button', { name: /beta/ }));
    expect(document.querySelector('[data-radix-scroll-area-viewport]')).not.toBe(viewport);
    expect(document.querySelector('[data-radix-scroll-area-viewport]')!.scrollTop).toBe(0);
    await act(async () => {});
  });

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
    const summaries = ['alpha', 'beta'].map(name => ({
      name, slug: name, source: 'owner/repo', detailUrl: `https://skills.sh/owner/repo/${name}`,
      isOfficial: false, displayMetric: { kind: 'installs', rawText: '10K', sortValue: 10000 },
    }));
    mocks.leaderboard.mockReset();
    mocks.leaderboard.mockResolvedValue(summaries);
    mocks.detail.mockReset();
    mocks.detail.mockImplementation(async (url: string) => ({
      ...summaries.find(skill => skill.detailUrl === url), highlights: [], securityAudits: [], installedOn: [],
      contentHtml: `<p>Content for ${url}</p>`,
    }));
    useLayoutStore.setState({ discoverListWidth: null });
    mocks.resizable.apply.mockClear();
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
