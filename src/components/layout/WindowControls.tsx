import { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Minus, X } from 'lucide-react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useOptionalWindowLifecycle } from '@/lifecycle/useWindowLifecycle';
import { useWindowMaximized } from '@/hooks/useWindowMaximized';
import { cn } from '@/lib/utils';

export function WindowControls({ className }: { className?: string }) {
  const { t } = useTranslation();
  const lifecycle = useOptionalWindowLifecycle();
  const isMaximized = useWindowMaximized();

  const appWindow = useMemo(() => {
    try {
      return getCurrentWebviewWindow();
    } catch {
      return null;
    }
  }, []);

  const handleMinimize = useCallback(() => {
    void appWindow?.minimize();
  }, [appWindow]);

  const handleToggleMaximize = useCallback(() => {
    void appWindow?.toggleMaximize();
  }, [appWindow]);

  const handleClose = useCallback(async () => {
    if (lifecycle) {
      await lifecycle.requestAction('quitApplication');
      return;
    }
    await appWindow?.close();
  }, [appWindow, lifecycle]);

  return (
    <div
      data-tauri-drag-region="false"
      className={cn('flex items-center h-full select-none shrink-0', className)}
    >
      <button
        type="button"
        onClick={handleMinimize}
        title={t('window.minimize')}
        aria-label={t('window.minimize')}
        className="inline-flex h-full w-10 items-center justify-center text-muted-foreground hover:bg-muted/80 hover:text-foreground active:bg-muted transition-colors cursor-pointer"
      >
        <Minus className="h-3.5 w-3.5" />
      </button>

      <button
        type="button"
        onClick={handleToggleMaximize}
        title={isMaximized ? t('window.restore') : t('window.maximize')}
        aria-label={isMaximized ? t('window.restore') : t('window.maximize')}
        className="inline-flex h-full w-10 items-center justify-center text-muted-foreground hover:bg-muted/80 hover:text-foreground active:bg-muted transition-colors cursor-pointer"
      >
        {isMaximized ? (
          <svg
            className="h-3.5 w-3.5"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            aria-hidden="true"
          >
            <rect x="4.5" y="1.5" width="9" height="9" rx="0.5" />
            <path d="M2.5 5.5v8a0.5 0.5 0 0 0 0.5 0.5h8" />
          </svg>
        ) : (
          <svg
            className="h-3.5 w-3.5"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            aria-hidden="true"
          >
            <rect x="2.5" y="2.5" width="11" height="11" rx="0.5" />
          </svg>
        )}
      </button>

      <button
        type="button"
        onClick={handleClose}
        title={t('window.close')}
        aria-label={t('window.close')}
        className="inline-flex h-full w-11 items-center justify-center text-muted-foreground hover:bg-destructive hover:text-destructive-foreground active:bg-destructive/90 transition-colors cursor-pointer"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}
