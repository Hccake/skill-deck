import { lazy, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';

// bundle-dynamic-imports: react-markdown ~40KB gzipped，仅在展示 release notes 时加载
const Markdown = lazy(() => import('react-markdown'));
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { ScrollArea } from '@/components/ui/scroll-area';
import { useUpdaterStore, relaunchApp } from '@/stores/updater';
import { useMutationInterruption } from '@/hooks/useMutationInterruption';
import { MutationInterruptionDialog } from '@/components/layout/MutationInterruptionDialog';

const RELEASE_URL = 'https://github.com/hccake/skill-deck/releases/latest';

export function UpdateDialog({ open }: { open: boolean }) {
  const { t } = useTranslation();
  const restartProtection = useMutationInterruption('restart', relaunchApp);
  const {
    status,
    newVersion,
    releaseNotes,
    downloadProgress,
    currentPlatform,
    downloadAndInstall,
    dismiss,
  } = useUpdaterStore();

  const isMac = currentPlatform === 'macos';

  // Dialog 不可关闭当下载中
  const handleOpenChange = (open: boolean) => {
    if (!open && status !== 'downloading') {
      dismiss();
    }
  };

  return (
    <>
      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogContent
        className="sm:max-w-lg"
        onInteractOutside={(e) => {
          if (status === 'downloading') e.preventDefault();
        }}
        onEscapeKeyDown={(e) => {
          if (status === 'downloading') e.preventDefault();
        }}
      >
        {/* rendering-conditional-render: 用显式三元避免 && 的 falsy 值风险 */}
        {status === 'available' ? (
          <>
            <DialogHeader>
              <DialogTitle>
                {t('settings.update.newVersionTitle', { version: newVersion })}
              </DialogTitle>
              <DialogDescription>
                {isMac
                  ? t('settings.update.availableDescMac')
                  : t('settings.update.availableDesc')}
              </DialogDescription>
            </DialogHeader>

            {releaseNotes ? (
              <ScrollArea className="max-h-[300px] rounded-md border p-4">
                <Suspense fallback={<div className="animate-pulse h-20 rounded bg-muted" />}>
                  <div className="prose prose-sm dark:prose-invert max-w-none">
                    <Markdown>{releaseNotes}</Markdown>
                  </div>
                </Suspense>
              </ScrollArea>
            ) : (
              <div className="flex items-center gap-2 py-4 text-sm text-muted-foreground">
                <a
                  href={RELEASE_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-1 text-primary hover:underline"
                >
                  {t('settings.update.noReleaseNotes')}
                  <ExternalLink className="h-3.5 w-3.5" />
                </a>
              </div>
            )}

            <DialogFooter>
              <Button
                variant="ghost"
                className="cursor-pointer"
                onClick={() => dismiss()}
              >
                {t('settings.update.later')}
              </Button>
              {isMac ? (
                <Button
                  className="cursor-pointer"
                  onClick={() => openUrl(RELEASE_URL)}
                >
                  {t('settings.update.goToDownload')}
                </Button>
              ) : (
                <Button
                  className="cursor-pointer"
                  onClick={() => downloadAndInstall()}
                >
                  {t('settings.update.updateNow')}
                </Button>
              )}
            </DialogFooter>
          </>
        ) : status === 'downloading' ? (
          <>
            <DialogHeader>
              <DialogTitle>
                {t('settings.update.updatingTitle', { version: newVersion })}
              </DialogTitle>
              <DialogDescription>
                {t('settings.update.downloadingProgress', {
                  version: newVersion,
                  progress: downloadProgress,
                })}
              </DialogDescription>
            </DialogHeader>

            <div className="py-4">
              <Progress value={downloadProgress} className="h-2" />
            </div>

            <DialogFooter>
              <Button
                variant="ghost"
                className="cursor-pointer"
                onClick={() => dismiss()}
              >
                {t('settings.update.cancel')}
              </Button>
            </DialogFooter>
          </>
        ) : status === 'ready' ? (
          <>
            <DialogHeader>
              <DialogTitle>
                {t('settings.update.readyTitle')}
              </DialogTitle>
              <DialogDescription>
                {t('settings.update.readyDesc', { version: newVersion })}
              </DialogDescription>
            </DialogHeader>

            <DialogFooter>
              <Button
                variant="ghost"
                className="cursor-pointer"
                onClick={() => dismiss()}
              >
                {t('settings.update.laterRestart')}
              </Button>
              <Button
                className="cursor-pointer"
                onClick={() => void restartProtection.requestAction()}
              >
                {t('settings.update.restartNow')}
              </Button>
            </DialogFooter>
          </>
        ) : null}
        </DialogContent>
      </Dialog>
      <MutationInterruptionDialog {...restartProtection.dialogProps} />
    </>
  );
}
