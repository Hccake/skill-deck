/* @vitest-environment jsdom */

import { renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { useWindowMaximized } from '../useWindowMaximized';

let resizeCallback: (() => void) | undefined;

const mockAppWindow = {
  isMaximized: vi.fn().mockResolvedValue(false),
  onResized: vi.fn().mockImplementation((cb: () => void) => {
    resizeCallback = cb;
    return Promise.resolve(vi.fn());
  }),
};

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => mockAppWindow,
}));

describe('useWindowMaximized', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resizeCallback = undefined;
    mockAppWindow.isMaximized.mockResolvedValue(false);
  });

  it('initializes with false and updates after query resolves', async () => {
    mockAppWindow.isMaximized.mockResolvedValue(true);
    const { result } = renderHook(() => useWindowMaximized());

    await waitFor(() => {
      expect(result.current).toBe(true);
    });
  });

  it('updates state when onResized fires', async () => {
    mockAppWindow.isMaximized.mockResolvedValue(false);
    const { result } = renderHook(() => useWindowMaximized());

    await waitFor(() => {
      expect(result.current).toBe(false);
    });

    mockAppWindow.isMaximized.mockResolvedValue(true);
    resizeCallback?.();

    await waitFor(() => {
      expect(result.current).toBe(true);
    });
  });
});
