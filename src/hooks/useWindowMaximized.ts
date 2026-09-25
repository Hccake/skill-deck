import { useEffect, useState } from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

export function useWindowMaximized(): boolean {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    try {
      const appWindow = getCurrentWebviewWindow();
      let unlisten: (() => void) | undefined;
      let active = true;

      void appWindow.isMaximized().then((max) => {
        if (active) setIsMaximized(max);
      }).catch(() => {});

      void appWindow.onResized(() => {
        void appWindow.isMaximized().then((max) => {
          if (active) setIsMaximized(max);
        }).catch(() => {});
      }).then((stop) => {
        if (active) unlisten = stop;
        else stop();
      }).catch(() => {});

      return () => {
        active = false;
        unlisten?.();
      };
    } catch {
      return;
    }
  }, []);

  return isMaximized;
}
