/* @vitest-environment jsdom */

import '@/test-utils';
import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { EnvironmentInfo, ProjectInfo } from '@/bindings';
import { useEnvironmentStore } from '@/stores/environment';
import { useProjectStore } from '@/stores/projects';

const mocks = vi.hoisted(() => ({
  listEnvironmentProjects: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  listEnvironmentProjects: (...args: unknown[]) => mocks.listEnvironmentProjects(...args),
}));

import { useEnvironmentProjects } from '../useEnvironmentProjects';

const host = { kind: 'host' as const };
const project: ProjectInfo = {
  binding: {
    id: 'project-a',
    nativePath: '/work/app',
    displayName: null,
    order: null,
    suppressCrossStorageWarning: false,
  },
  storage: { access: 'native', owner: null },
};

const environmentInfo: EnvironmentInfo = {
  environment: host,
  displayName: 'Host',
  status: 'available',
  revision: 1,
  error: null,
};

describe('useEnvironmentProjects', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useEnvironmentStore.setState({ environments: [environmentInfo] });
    useProjectStore.setState({
      projectsByEnvironment: {},
      loadStateByEnvironment: {},
      errorsByEnvironment: {},
    });
  });

  it('refreshes an idle available Environment and exposes the snapshot', async () => {
    mocks.listEnvironmentProjects.mockResolvedValue([project]);

    const { result } = renderHook(() => useEnvironmentProjects(host));

    await waitFor(() => expect(result.current.loadState).toBe('ready'));
    expect(result.current.projects).toEqual([project]);
    expect(mocks.listEnvironmentProjects).toHaveBeenCalledWith(host);
  });

  it('does not refresh while a workspace transition is active', async () => {
    renderHook(() => useEnvironmentProjects(host, { transitionActive: true }));

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(mocks.listEnvironmentProjects).not.toHaveBeenCalled();
    expect(useProjectStore.getState().loadStateByEnvironment).toEqual({});
  });

  it('exposes the Store error and allows an explicit retry', async () => {
    mocks.listEnvironmentProjects
      .mockRejectedValueOnce(new Error('project load failed'))
      .mockResolvedValueOnce([project]);

    const { result } = renderHook(() => useEnvironmentProjects(host));

    await waitFor(() => expect(result.current.loadState).toBe('error'));
    expect(result.current.error).toEqual({
      kind: 'custom',
      data: { message: 'project load failed' },
    });

    await act(async () => {
      await result.current.refresh();
    });

    expect(result.current.loadState).toBe('ready');
    expect(result.current.projects).toEqual([project]);
    expect(mocks.listEnvironmentProjects).toHaveBeenCalledTimes(2);
  });

  it('does not refresh an unavailable Environment', async () => {
    useEnvironmentStore.setState({
      environments: [{ ...environmentInfo, status: 'unavailable' }],
    });

    renderHook(() => useEnvironmentProjects(host));

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(mocks.listEnvironmentProjects).not.toHaveBeenCalled();
  });

  it('keeps the newest explicit refresh result when an older request resolves later', async () => {
    let resolveFirst: ((projects: ProjectInfo[]) => void) | undefined;
    let resolveSecond: ((projects: ProjectInfo[]) => void) | undefined;
    const firstProject = { ...project, binding: { ...project.binding, id: 'first' } };
    const secondProject = { ...project, binding: { ...project.binding, id: 'second' } };
    mocks.listEnvironmentProjects
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveSecond = resolve; }));

    const { result } = renderHook(() => useEnvironmentProjects(host));
    await waitFor(() => expect(mocks.listEnvironmentProjects).toHaveBeenCalledTimes(1));

    let secondRefresh: Promise<ProjectInfo[]> | undefined;
    await act(async () => {
      secondRefresh = result.current.refresh();
    });
    await waitFor(() => expect(mocks.listEnvironmentProjects).toHaveBeenCalledTimes(2));

    await act(async () => {
      resolveSecond?.([secondProject]);
      await secondRefresh;
    });
    await act(async () => {
      resolveFirst?.([firstProject]);
    });

    await waitFor(() => expect(result.current.projects).toEqual([secondProject]));
    expect(result.current.loadState).toBe('ready');
  });
});
