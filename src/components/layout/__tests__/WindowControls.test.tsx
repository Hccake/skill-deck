/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { WindowLifecycleContext } from '@/lifecycle/useWindowLifecycle';
import { WindowControls } from '../WindowControls';

const mockAppWindow = {
  minimize: vi.fn().mockResolvedValue(undefined),
  toggleMaximize: vi.fn().mockResolvedValue(undefined),
  isMaximized: vi.fn().mockResolvedValue(false),
  close: vi.fn().mockResolvedValue(undefined),
  onResized: vi.fn().mockResolvedValue(vi.fn()),
};

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => mockAppWindow,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const translations: Record<string, string> = {
        'window.minimize': '最小化',
        'window.maximize': '最大化',
        'window.restore': '还原',
        'window.close': '关闭',
      };
      return translations[key] ?? key;
    },
  }),
}));

describe('WindowControls', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockAppWindow.isMaximized.mockResolvedValue(false);
  });

  it('renders minimize, maximize, and close buttons', () => {
    render(<WindowControls />);

    expect(screen.getByRole('button', { name: '最小化' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '最大化' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '关闭' })).toBeTruthy();
  });

  it('calls appWindow.minimize when minimize button is clicked', () => {
    render(<WindowControls />);
    const minimizeBtn = screen.getByRole('button', { name: '最小化' });

    fireEvent.click(minimizeBtn);
    expect(mockAppWindow.minimize).toHaveBeenCalledTimes(1);
  });

  it('calls appWindow.toggleMaximize when maximize button is clicked', () => {
    render(<WindowControls />);
    const maximizeBtn = screen.getByRole('button', { name: '最大化' });

    fireEvent.click(maximizeBtn);
    expect(mockAppWindow.toggleMaximize).toHaveBeenCalledTimes(1);
  });

  it('updates label to restore when window is maximized', async () => {
    mockAppWindow.isMaximized.mockResolvedValue(true);
    render(<WindowControls />);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: '还原' })).toBeTruthy();
    });
  });

  it('delegates close to window lifecycle when available', () => {
    const mockRequestAction = vi.fn().mockResolvedValue(undefined);
    render(
      <WindowLifecycleContext.Provider value={{ requestAction: mockRequestAction }}>
        <WindowControls />
      </WindowLifecycleContext.Provider>,
    );

    const closeBtn = screen.getByRole('button', { name: '关闭' });
    fireEvent.click(closeBtn);

    expect(mockRequestAction).toHaveBeenCalledWith('quitApplication');
    expect(mockAppWindow.close).not.toHaveBeenCalled();
  });

  it('falls back to appWindow.close when window lifecycle is not available', () => {
    render(
      <WindowLifecycleContext.Provider value={null}>
        <WindowControls />
      </WindowLifecycleContext.Provider>,
    );

    const closeBtn = screen.getByRole('button', { name: '关闭' });
    fireEvent.click(closeBtn);

    expect(mockAppWindow.close).toHaveBeenCalledTimes(1);
  });
});
