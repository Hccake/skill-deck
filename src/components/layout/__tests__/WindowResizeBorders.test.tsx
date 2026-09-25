/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { WindowResizeBorders } from '../WindowResizeBorders';

const mockAppWindow = {
  isMaximized: vi.fn().mockResolvedValue(false),
  onResized: vi.fn().mockResolvedValue(vi.fn()),
  startResizeDragging: vi.fn().mockResolvedValue(undefined),
};

vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => mockAppWindow,
}));

describe('WindowResizeBorders', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockAppWindow.isMaximized.mockResolvedValue(false);
  });

  it('renders all 8 resize handles on Windows when not maximized', () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    );

    render(<WindowResizeBorders />);

    expect(screen.getByTestId('resize-north')).toBeTruthy();
    expect(screen.getByTestId('resize-south')).toBeTruthy();
    expect(screen.getByTestId('resize-west')).toBeTruthy();
    expect(screen.getByTestId('resize-east')).toBeTruthy();
    expect(screen.getByTestId('resize-north-west')).toBeTruthy();
    expect(screen.getByTestId('resize-north-east')).toBeTruthy();
    expect(screen.getByTestId('resize-south-west')).toBeTruthy();
    expect(screen.getByTestId('resize-south-east')).toBeTruthy();

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });

  it('triggers startResizeDragging with direction on mousedown', () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    );

    render(<WindowResizeBorders />);

    const northHandle = screen.getByTestId('resize-north');
    fireEvent.mouseDown(northHandle, { button: 0 });
    expect(mockAppWindow.startResizeDragging).toHaveBeenCalledWith('North');

    const southEastHandle = screen.getByTestId('resize-south-east');
    fireEvent.mouseDown(southEastHandle, { button: 0 });
    expect(mockAppWindow.startResizeDragging).toHaveBeenCalledWith('SouthEast');

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });

  it('does not render on macOS', () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36',
    );

    const { container } = render(<WindowResizeBorders />);
    expect(container.firstChild).toBeNull();

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });

  it('does not render on Linux (uses native window resizing)', () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)',
    );

    const { container } = render(<WindowResizeBorders />);
    expect(container.firstChild).toBeNull();

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });

  it('does not render when window is maximized', async () => {
    const originalUserAgent = navigator.userAgent;
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36',
    );
    mockAppWindow.isMaximized.mockResolvedValue(true);

    const { container } = render(<WindowResizeBorders />);

    await waitFor(() => {
      expect(container.firstChild).toBeNull();
    });

    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(originalUserAgent);
  });
});
