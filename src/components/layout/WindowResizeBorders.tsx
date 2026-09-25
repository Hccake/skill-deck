import { useMemo } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { isWindows } from '@/lib/platform';
import { useWindowMaximized } from '@/hooks/useWindowMaximized';

type ResizeDirection =
  | 'North'
  | 'South'
  | 'East'
  | 'West'
  | 'NorthEast'
  | 'NorthWest'
  | 'SouthEast'
  | 'SouthWest';

export function WindowResizeBorders() {
  const windows = isWindows();
  const isMaximized = useWindowMaximized();

  const appWindow = useMemo(() => {
    try {
      return getCurrentWebviewWindow();
    } catch {
      return null;
    }
  }, []);

  // Only Windows frameless window needs manual resize borders; macOS and Linux use native window resizing
  if (!windows || isMaximized || !appWindow) {
    return null;
  }

  const handleResize = (direction: ResizeDirection) => (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    void appWindow.startResizeDragging(direction);
  };

  return (
    <div
      data-testid="window-resize-borders"
      data-tauri-drag-region="false"
      className="pointer-events-none fixed inset-0 z-50 overflow-hidden select-none"
    >
      {/* Edges */}
      <div
        data-testid="resize-north"
        onMouseDown={handleResize('North')}
        className="pointer-events-auto absolute top-0 left-2 right-2 h-1 cursor-n-resize"
      />
      <div
        data-testid="resize-south"
        onMouseDown={handleResize('South')}
        className="pointer-events-auto absolute bottom-0 left-2 right-2 h-1.5 cursor-s-resize"
      />
      <div
        data-testid="resize-west"
        onMouseDown={handleResize('West')}
        className="pointer-events-auto absolute top-2 bottom-2 left-0 w-1.5 cursor-w-resize"
      />
      <div
        data-testid="resize-east"
        onMouseDown={handleResize('East')}
        className="pointer-events-auto absolute top-2 bottom-2 right-0 w-1.5 cursor-e-resize"
      />

      {/* Corners */}
      <div
        data-testid="resize-north-west"
        onMouseDown={handleResize('NorthWest')}
        className="pointer-events-auto absolute top-0 left-0 h-2.5 w-2.5 cursor-nwse-resize"
      />
      <div
        data-testid="resize-north-east"
        onMouseDown={handleResize('NorthEast')}
        className="pointer-events-auto absolute top-0 right-0 h-2.5 w-2.5 cursor-nesw-resize"
      />
      <div
        data-testid="resize-south-west"
        onMouseDown={handleResize('SouthWest')}
        className="pointer-events-auto absolute bottom-0 left-0 h-2.5 w-2.5 cursor-nesw-resize"
      />
      <div
        data-testid="resize-south-east"
        onMouseDown={handleResize('SouthEast')}
        className="pointer-events-auto absolute bottom-0 right-0 h-2.5 w-2.5 cursor-nwse-resize"
      />
    </div>
  );
}
