/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { LibraryPage } from '../LibraryPage';
import type { LibraryWorkspaceState } from '@/lib/libraries/workspace';
import {
  checkLibrarySkillUpdates,
  previewLibrarySkillUpdates,
  readLibrarySkillContent,
  updateLibrarySkills,
} from '@/hooks/useTauriApi';
import type { EnvironmentRef } from '@/bindings';

const execute = vi.hoisted(() => vi.fn());
const addDialog = vi.hoisted(() => ({ props: null as Record<string, unknown> | null }));
const contextState = vi.hoisted(() => ({
  selectedContext: {
    environment: { kind: 'native' as const },
    scope: { scope: 'global' as const },
  },
}));

function LocationProbe() {
  return <span data-testid="location-search">{useLocation().search}</span>;
}
const workspaceView = vi.hoisted(() => ({
  catalog: {
    environment: { kind: 'native' as const },
    libraries: [{ id: 'lib-1', name: 'Backend', skillCount: 1 }],
    revision: 'catalog-1',
    usageProjection: [],
  },
  selectedLibraryId: 'lib-1',
  detail: {
    id: 'lib-1',
    name: 'Backend',
    skills: [{
      name: 'api-design',
      description: 'Design APIs',
      source: 'https://example.com/repo',
      sourceType: 'git',
      sourceUrl: 'https://example.com/repo',
      skillPath: 'skills/api-design',
      pluginName: null,
      refName: null,
      contentHash: 'old-hash',
      updatedAt: null,
    }],
    usages: [],
  },
  detailPhase: 'ready',
  detailError: null,
}) as Pick<
  LibraryWorkspaceState,
  'catalog' | 'selectedLibraryId' | 'detail' | 'detailPhase' | 'detailError'
>);

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector(contextState),
}));

vi.mock('@/stores/environment', () => ({
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: [{
      environment: { kind: 'native' },
      displayName: 'Windows',
      status: 'available',
      revision: 1,
      error: null,
    }],
  }),
}));

vi.mock('@/components/library', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/components/library')>(),
  LibraryAddDialog: (props: Record<string, unknown>) => {
    addDialog.props = props;
    return <div role="dialog" aria-label="library-add-dialog" />;
  },
}));

vi.mock('@/hooks/useLibraryWorkspace', () => ({
  useLibraryWorkspace: () => ({
    environment: { kind: 'native' },
    phase: 'ready',
    catalog: workspaceView.catalog,
    selectedLibraryId: workspaceView.selectedLibraryId,
    detail: workspaceView.detail,
    detailPhase: workspaceView.detailPhase,
    detailError: workspaceView.detailError,
    catalogError: null,
    pendingAdd: null,
    retryAdd: null,
    lastAddResults: [],
    version: 1,
    execute,
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  checkLibrarySkillUpdates: vi.fn(),
  readLibrarySkillContent: vi.fn(),
  removeLibrarySkill: vi.fn(),
  previewLibrarySkillUpdates: vi.fn(),
  updateLibrarySkills: vi.fn(),
}));

describe('LibraryPage maintenance', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    addDialog.props = null;
    workspaceView.catalog = {
      environment: { kind: 'native' },
      libraries: [{ id: 'lib-1', name: 'Backend', skillCount: 1 }],
      revision: 'catalog-1',
      usageProjection: [],
    };
    workspaceView.selectedLibraryId = 'lib-1';
    workspaceView.detail = {
      id: 'lib-1',
      name: 'Backend',
      skills: [{
        name: 'api-design',
        description: 'Design APIs',
        source: 'https://example.com/repo',
        sourceType: 'git',
        sourceUrl: 'https://example.com/repo',
        skillPath: 'skills/api-design',
        pluginName: null,
        refName: null,
        contentHash: 'old-hash',
        updatedAt: null,
      }],
      usages: [],
    };
    workspaceView.detailPhase = 'ready';
    workspaceView.detailError = null;
    (contextState.selectedContext as { environment: EnvironmentRef }).environment = { kind: 'native' };
    execute.mockResolvedValue({ status: 'succeeded', snapshot: {} });
    vi.mocked(readLibrarySkillContent).mockResolvedValue('# API design');
    vi.mocked(checkLibrarySkillUpdates).mockResolvedValue({
      outcome: 'completed',
      sources: [],
      skills: [{
        name: 'api-design',
        source: 'https://example.com/repo',
        hasUpdate: true,
        status: 'updateAvailable',
        capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
        reason: null,
        gitRef: null,
        sourceUrl: 'https://example.com/repo',
        skillPath: 'skills/api-design',
        freshness: 'fresh',
      }],
    });
    vi.mocked(previewLibrarySkillUpdates).mockResolvedValue({
      token: { generation: 'preview-1' },
      skillNames: ['api-design'],
    });
    vi.mocked(updateLibrarySkills).mockResolvedValue({
      status: 'completed',
      response: {
        sources: [],
        results: [{
          skillName: 'api-design',
          status: 'succeeded',
          sourceResultId: 'source-1',
          contentCommit: 'succeeded',
          catalogCommit: 'succeeded',
          error: null,
        }],
        outcome: 'succeeded',
        library: {
          id: 'lib-1',
          name: 'Backend',
          skills: [],
          usages: [],
        },
      },
    });
  });

  it('checks metadata first and submits one Library update after confirmation', async () => {
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);

    fireEvent.click(screen.getByRole('button', { name: 'libraries.checkUpdates' }));
    await waitFor(() => expect(checkLibrarySkillUpdates).toHaveBeenCalledWith(
      { kind: 'native' },
      'lib-1',
    ));
    expect(await screen.findByText('skills.updateStatusLabel.available')).toBeTruthy();
    expect(screen.getByRole('status', {
      name: 'libraries.updateSummary.updateAvailable:{"count":1}',
    })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'libraries.update' }));
    expect(await screen.findByText(/libraries.confirmUpdateDescription/)).toBeTruthy();
    expect(previewLibrarySkillUpdates).toHaveBeenCalledWith({
      environment: { kind: 'native' },
      libraryId: 'lib-1',
      skillNames: ['api-design'],
    });
    fireEvent.click(screen.getAllByRole('button', { name: 'libraries.update' }).at(-1)!);

    await waitFor(() => expect(updateLibrarySkills).toHaveBeenCalledWith({
      request: {
        environment: { kind: 'native' },
        libraryId: 'lib-1',
        skillNames: ['api-design'],
      },
      expectedToken: { generation: 'preview-1' },
      continuation: null,
      riskConfirmation: null,
    }));
  });

  it('captures the current Library target and existing members when opening add flow', () => {
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);
    fireEvent.click(screen.getByRole('button', { name: 'libraries.addSkill' }));

    expect(screen.getByRole('dialog', { name: 'library-add-dialog' })).toBeTruthy();
    expect(addDialog.props?.target).toEqual({
      environment: { kind: 'native' },
      environmentName: 'Windows',
      libraryId: 'lib-1',
      libraryName: 'Backend',
    });
    expect([...(addDialog.props?.existingSkillNames as Set<string>)]).toEqual(['api-design']);
    expect(addDialog.props?.execute).toBe(execute);
  });

  it('selects the Library requested by a Skills-page application link', async () => {
    execute.mockResolvedValueOnce({
      status: 'succeeded',
      snapshot: {
        catalog: {
          environment: { kind: 'native' },
          libraries: [
            { id: 'lib-1', name: 'Backend', skillCount: 1 },
            { id: 'lib-2', name: 'Frontend', skillCount: 1 },
          ],
          revision: 'catalog-2',
        },
        selectedLibraryId: 'lib-1',
      },
    }).mockResolvedValueOnce({ status: 'succeeded', snapshot: {} });

    render(<MemoryRouter initialEntries={['/libraries?library=lib-2']}><LibraryPage /></MemoryRouter>);

    await waitFor(() => expect(execute).toHaveBeenCalledWith({ kind: 'select', libraryId: 'lib-2' }));
  });

  it('confirms a completed check that found nothing', async () => {
    vi.mocked(checkLibrarySkillUpdates).mockResolvedValue({
      outcome: 'completed',
      sources: [],
      skills: [{
        name: 'api-design',
        source: 'https://example.com/repo',
        hasUpdate: false,
        status: 'upToDate',
        capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
        reason: null,
        gitRef: null,
        sourceUrl: 'https://example.com/repo',
        skillPath: 'skills/api-design',
        freshness: 'fresh',
      }],
    });

    render(<MemoryRouter><LibraryPage /></MemoryRouter>);
    fireEvent.click(screen.getByRole('button', { name: 'libraries.checkUpdates' }));

    // 全是最新时，摘要文字之外还需要一个就地的完成反馈，否则点击几乎没有可见变化。
    expect(await screen.findByText('skills.checkCompleted')).toBeTruthy();
  });

  it('switches the list to compact navigation once a Skill is selected', async () => {
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);

    const removeName = 'libraries.removeSkill:{"name":"api-design"}';

    // 未选中时是完整卡片：有标识列，也有行内移除按钮。
    expect(screen.getByTestId('library-skill-marker')).toBeTruthy();
    expect(screen.getAllByRole('button', { name: removeName })).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'common.close' })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'api-design' }));

    // 选中后列表退居为导航，卡片和它的行内操作一起消失。
    await waitFor(() => expect(screen.queryByTestId('library-skill-marker')).toBeNull());
    // 详情面板接管，移除动作仍然只有一处——没有在列表和详情之间重复。
    expect(screen.getByRole('button', { name: 'common.close' })).toBeTruthy();
    expect(screen.getAllByRole('button', { name: removeName })).toHaveLength(1);
  });

  it('loads the selected Skill content from the Library storage', async () => {
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);

    fireEvent.click(screen.getByRole('button', { name: 'api-design' }));

    await waitFor(() => expect(readLibrarySkillContent).toHaveBeenCalledWith(
      { kind: 'native' },
      'lib-1',
      'api-design',
    ));
    expect(await screen.findByText('API design')).toBeTruthy();
  });

  it('keeps the Library member count stable while search reports its own result count', () => {
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);

    expect(screen.getByText('libraries.skillCount:{"count":1}')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Backend' })).toBeTruthy();

    fireEvent.change(screen.getByRole('searchbox', { name: 'libraries.searchSkills' }), {
      target: { value: 'missing' },
    });

    expect(screen.getByText('libraries.skillCount:{"count":1}')).toBeTruthy();
    expect(screen.getByText('libraries.searchResultCount:{"visible":0,"total":1}')).toBeTruthy();
  });

  it('opens deletion for the clicked Library instead of the selected Library', () => {
    workspaceView.catalog = {
      environment: { kind: 'native' },
      libraries: [
        { id: 'lib-a', name: 'Selected', skillCount: 1 },
        { id: 'lib-b', name: 'Clicked', skillCount: 4 },
      ],
      revision: 'catalog-delete-target',
      usageProjection: [],
    };
    workspaceView.selectedLibraryId = 'lib-a';
    workspaceView.detail = { id: 'lib-a', name: 'Selected', skills: [], usages: [] };
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);

    fireEvent.click(screen.getByRole('button', {
      name: 'libraries.deleteNamed:{"name":"Clicked"}',
    }));

    expect(screen.getByText('libraries.deleteLibraryTitle:{"name":"Clicked"}')).toBeTruthy();
    expect(screen.getByText('libraries.deleteLibraryDescriptionWithCount:{"count":4}')).toBeTruthy();
  });

  it('does not open deletion for a Library with an applied or pending usage', () => {
    workspaceView.catalog = {
      environment: { kind: 'native' },
      libraries: [
        { id: 'lib-a', name: 'Selected', skillCount: 1 },
        { id: 'lib-b', name: 'Locked', skillCount: 4 },
      ],
      revision: 'catalog-locked-delete',
      usageProjection: [{ libraryId: 'lib-b', confirmedCount: 1, pendingCount: 0 }],
    };
    workspaceView.selectedLibraryId = 'lib-a';
    workspaceView.detail = { id: 'lib-a', name: 'Selected', skills: [], usages: [] };
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);

    fireEvent.click(screen.getByRole('button', {
      name: 'libraries.deleteNamed:{"name":"Locked"}',
    }));

    expect(screen.queryByText('libraries.deleteLibraryTitle:{"name":"Locked"}')).toBeNull();
  });

  it('replaces an invalid deep link with the committed fallback selection', async () => {
    execute.mockResolvedValueOnce({
      status: 'succeeded',
      snapshot: {
        ...workspaceView,
        phase: 'ready',
        catalogError: null,
        pendingAdd: null,
        retryAdd: null,
        lastAddResults: [],
        version: 2,
      },
    });
    render(
      <MemoryRouter initialEntries={['/libraries?library=missing']}>
        <LibraryPage />
        <LocationProbe />
      </MemoryRouter>,
    );

    await waitFor(() => expect(screen.getByTestId('location-search').textContent)
      .toBe('?library=lib-1'));
  });

  it('keeps the catalog visible and retries only the selected detail after deletion', () => {
    workspaceView.detail = null as never;
    workspaceView.detailPhase = 'error';
    workspaceView.detailError = { kind: 'staleTarget' } as never;
    render(<MemoryRouter><LibraryPage /></MemoryRouter>);

    expect(screen.getByText('libraries.detailLoadError')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));

    expect(execute).toHaveBeenCalledWith({ kind: 'select', libraryId: 'lib-1' });
  });

});

describe('LibraryPage split layout', () => {
  it('sizes both panels with percentages', () => {
    // react-resizable-panels v4 把裸数字当像素：`maxSize={60}` 会把列表钳到 60px 宽且拖不动。
    // 这个失效是静默的（不报错、类型也合法），所以在源码层面守住百分比写法。
    const sources = import.meta.glob('../LibraryPage.tsx', {
      query: '?raw',
      import: 'default',
      eager: true,
    }) as Record<string, string>;
    const source = Object.values(sources)[0];
    const sizeProps = source.match(/(?:default|min|max)Size=\{?[^\n]*/g) ?? [];

    expect(sizeProps.length).toBeGreaterThan(0);
    for (const prop of sizeProps) {
      expect(prop).toMatch(/%/);
    }
  });
});
