import { lazy, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink } from 'lucide-react';

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
import { useUpdaterStore } from '@/stores/updater';
import { useWindowLifecycle } from '@/lifecycle/useWindowLifecycle';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';

const RELEASE_URL = 'https://github.com/hccake/skill-deck/releases/latest';

export function UpdateDialog({ open }: { open: boolean }) {
  const { t } = useTranslation();
  const writeBlocked = useBusinessWriteBlocked();
  const { requestAction } = useWindowLifecycle();
  const {
    status,
    newVersion,
    releaseNotes,
    downloadProgress,
    failedOperation,
    downloadAndInstall,
    retry,
    dismiss,
  } = useUpdaterStore();

  const handleOpenChange = (open: boolean) => {
    if (!open) dismiss();
  };

  return (
    <>
      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogContent
          className="sm:max-w-lg"
          dismissible={status !== 'downloading'}
          closeLabel={t('common.close')}
          aria-busy={status === 'downloading'}
        >
        {/* rendering-conditional-render: 用显式三元避免 && 的 falsy 值风险 */}
        {status === 'available' ? (
          <>
            <DialogHeader>
              <DialogTitle>
                {t('settings.update.newVersionTitle', { version: newVersion })}
              </DialogTitle>
              <DialogDescription>
                {t('settings.update.availableDesc')}
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
                  <ExternalLink className="h-3.5 w-3.5" aria-hidden="true" />
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
              <Button
                className="cursor-pointer"
                disabled={writeBlocked}
                onClick={() => downloadAndInstall()}
              >
                {t('settings.update.updateNow')}
              </Button>
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

            <div className="py-4" role="status" aria-live="polite">
              <Progress
                value={downloadProgress}
                aria-label={t('settings.update.downloadProgressLabel')}
                aria-valuenow={downloadProgress}
                className="h-2"
              />
            </div>

            <DialogFooter>
              <Button
                variant="ghost"
                className="cursor-pointer"
                onClick={() => dismiss()}
              >
                {t('settings.update.hide')}
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
                disabled={writeBlocked}
                onClick={() => void requestAction('restartApplication')}
              >
                {t('settings.update.restartNow')}
              </Button>
            </DialogFooter>
          </>
        ) : status === 'error' ? (
          <>
            <DialogHeader>
              <DialogTitle>{t('settings.update.errorTitle')}</DialogTitle>
              <DialogDescription>{t('settings.update.errorDescription')}</DialogDescription>
            </DialogHeader>

            <div role="alert" className="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
              {failedOperation === 'install'
                ? t('settings.update.installError')
                : t('settings.update.checkError')}
            </div>

            <DialogFooter>
              <Button variant="ghost" onClick={() => dismiss()}>
                {t('settings.update.hide')}
              </Button>
              <Button
                disabled={writeBlocked && failedOperation === 'install'}
                onClick={() => void retry()}
              >
                {t('settings.update.retry')}
              </Button>
            </DialogFooter>
          </>
        ) : null}
        </DialogContent>
      </Dialog>
    </>
  );
}
