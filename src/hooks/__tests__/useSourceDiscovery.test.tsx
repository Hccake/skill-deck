/* @vitest-environment jsdom */

import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useSourceDiscovery } from '../useSourceDiscovery';

const apiMocks = vi.hoisted(() => ({
  discoverSkillSource: vi.fn(),
}));
const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listeners: [] as Array<(event: { payload: unknown }) => void>,
}));

vi.mock('@/hooks/useTauriApi', () => apiMocks);
vi.mock('@tauri-apps/api/event', () => eventMocks);

const environment = { kind: 'native' } as const;
const discoverySession = {
  sessionId: 'discovery-1',
  environment,
  sourceFingerprint: 'source-1',
  expiresAtEpochMs: 1000,
} as const;

function fetchResult(name: string) {
  return {
    discoverySession,
    sourceType: 'github' as const,
    sourceUrl: `https://github.com/owner/${name}`,
    gitRef: null,
    skillFilter: null,
    skills: [{
      name,
      installDirName: name,
      description: name,
      relativePath: `${name}/SKILL.md`,
    }],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('useSourceDiscovery', () => {
  beforeEach(() => {
    apiMocks.discoverSkillSource.mockReset();
    eventMocks.listen.mockReset();
    eventMocks.listeners.length = 0;
    eventMocks.listen.mockImplementation((_eventName, listener) => {
      eventMocks.listeners.push(listener);
      return Promise.resolve(() => undefined);
    });
  });

  it('discovers the normalized source and exposes the resolved selection', async () => {
    apiMocks.discoverSkillSource.mockResolvedValue(fetchResult('alpha'));
    const { result } = renderHook(() => useSourceDiscovery(environment));

    await act(async () => {
      await result.current.discover('skills add owner/repo --skill alpha --agent codex');
    });

    expect(apiMocks.discoverSkillSource).toHaveBeenCalledWith(
      environment,
      'owner/repo',
      expect.any(String),
      { wildcardRequested: false, explicitSkillNames: ['alpha'] },
    );
    expect(result.current.status).toBe('success');
    expect(result.current.selection?.selectedSkillNames).toEqual(['alpha']);
    expect(result.current.selection?.agentSelectionIntent.explicitAgentIds).toEqual(['codex']);
  });

  it('keeps progress and completion isolated by operation ID', async () => {
    const first = deferred<ReturnType<typeof fetchResult>>();
    const second = deferred<ReturnType<typeof fetchResult>>();
    apiMocks.discoverSkillSource
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useSourceDiscovery(environment));

    let firstRequest!: Promise<unknown>;
    let secondRequest!: Promise<unknown>;
    act(() => {
      firstRequest = result.current.discover('owner/first');
    });
    await waitFor(() => expect(apiMocks.discoverSkillSource).toHaveBeenCalledTimes(1));
    const firstOperationId = apiMocks.discoverSkillSource.mock.calls[0][2];

    act(() => {
      secondRequest = result.current.discover('owner/second');
    });
    await waitFor(() => expect(apiMocks.discoverSkillSource).toHaveBeenCalledTimes(2));
    const secondOperationId = apiMocks.discoverSkillSource.mock.calls[1][2];

    act(() => {
      eventMocks.listeners[0]({
        payload: {
          operation_id: firstOperationId,
          phase: 'cloning',
          elapsed_secs: 20,
          timeout_secs: 120,
          message: null,
        },
      });
    });
    expect(result.current.cloneProgress).toBeNull();

    act(() => {
      eventMocks.listeners[0]({
        payload: {
          operation_id: secondOperationId,
          phase: 'cloning',
          elapsed_secs: 2,
          timeout_secs: 120,
          message: null,
        },
      });
    });
    expect(result.current.cloneProgress?.elapsed_secs).toBe(2);

    await act(async () => {
      first.resolve(fetchResult('first'));
      await firstRequest;
    });
    expect(result.current.status).toBe('loading');

    await act(async () => {
      second.resolve(fetchResult('second'));
      await secondRequest;
    });
    expect(result.current.status).toBe('success');
    expect(result.current.result?.skills[0].name).toBe('second');
  });

  it('exposes a failed request and retries the same source with a new operation ID', async () => {
    apiMocks.discoverSkillSource
      .mockRejectedValueOnce({ kind: 'gitNetworkError', data: { message: 'offline' } })
      .mockResolvedValueOnce(fetchResult('retry'));
    const { result } = renderHook(() => useSourceDiscovery(environment));

    await act(async () => {
      await result.current.discover('owner/retry');
    });
    const firstOperationId = apiMocks.discoverSkillSource.mock.calls[0][2];
    expect(result.current.status).toBe('error');
    expect(result.current.error?.kind).toBe('gitNetworkError');

    await act(async () => {
      await result.current.retry();
    });
    expect(apiMocks.discoverSkillSource).toHaveBeenCalledTimes(2);
    expect(apiMocks.discoverSkillSource.mock.calls[1][1]).toBe('owner/retry');
    expect(apiMocks.discoverSkillSource.mock.calls[1][2]).not.toBe(firstOperationId);
    expect(result.current.status).toBe('success');
  });
});
