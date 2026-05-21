/* @vitest-environment jsdom */

import '@/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { fetch } from '@tauri-apps/plugin-http';
import { SkillSearch } from '../SkillSearch';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@tauri-apps/plugin-http', () => ({
  fetch: vi.fn(),
}));

const fetchMock = vi.mocked(fetch);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function searchResponse(name: string) {
  return {
    ok: true,
    json: vi.fn().mockResolvedValue({
      skills: [{
        id: name,
        name,
        installs: 1,
        source: `owner/${name}`,
      }],
    }),
  } as Pick<Response, 'ok' | 'json'> as Response;
}

async function flushAsyncWork() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('SkillSearch', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    fetchMock.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('ignores stale results when an earlier search resolves after a newer query', async () => {
    const firstSearch = deferred<ReturnType<typeof searchResponse>>();
    const secondSearch = deferred<ReturnType<typeof searchResponse>>();

    fetchMock
      .mockReturnValueOnce(firstSearch.promise as ReturnType<typeof fetch>)
      .mockReturnValueOnce(secondSearch.promise as ReturnType<typeof fetch>);

    render(<SkillSearch installedSkillKeys={new Set()} onInstall={vi.fn()} />);

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'ab' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'abc' } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });

    await act(async () => {
      secondSearch.resolve(searchResponse('new-result'));
    });
    await flushAsyncWork();

    expect(screen.getByText('new-result')).toBeTruthy();

    await act(async () => {
      firstSearch.resolve(searchResponse('old-result'));
    });
    await flushAsyncWork();

    expect(screen.queryByText('old-result')).toBeNull();
    expect(screen.getByText('new-result')).toBeTruthy();
  });
});
