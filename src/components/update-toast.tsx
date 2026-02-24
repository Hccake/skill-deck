import { toast } from 'sonner';
import { type TFunction } from 'i18next';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { relaunchApp } from '@/stores/updater';

const UPDATE_TOAST_ID = 'app-update';

/** 显示下载进度 toast（Windows/Linux） */
export function showDownloadingToast(version: string, progress: number, t: TFunction) {
  toast.custom(
    () => (
      <div className="flex flex-col gap-2 w-full">
        <p className="text-sm font-medium">
          {t('settings.update.downloading', { version })}
        </p>
        <Progress value={progress} className="h-2" />
      </div>
    ),
    { id: UPDATE_TOAST_ID, duration: Infinity }
  );
}

/** 显示更新就绪 toast（Windows/Linux） */
export function showReadyToast(version: string, onDismiss: () => void, t: TFunction) {
  toast.custom(
    () => (
      <div className="flex flex-col gap-3 w-full">
        <div>
          <p className="text-sm font-medium">
            {t('settings.update.newVersion', { version })}
          </p>
          <p className="text-xs text-muted-foreground mt-0.5">
            {t('settings.update.newVersionHint')}
          </p>
        </div>
        <div className="flex justify-end gap-2">
          <Button
            variant="ghost"
            size="sm"
            className="cursor-pointer"
            onClick={() => {
              toast.dismiss(UPDATE_TOAST_ID);
              onDismiss();
            }}
          >
            {t('settings.update.later')}
          </Button>
          <Button
            size="sm"
            className="cursor-pointer"
            onClick={() => relaunchApp()}
          >
            {t('settings.update.restartNow')}
          </Button>
        </div>
      </div>
    ),
    { id: UPDATE_TOAST_ID, duration: Infinity }
  );
}

/** 显示 macOS 发现新版本 toast */
export function showMacUpdateToast(version: string, onDismiss: () => void, t: TFunction) {
  const releaseUrl = 'https://github.com/hccake/skill-deck/releases/latest';
  toast.custom(
    () => (
      <div className="flex flex-col gap-3 w-full">
        <div>
          <p className="text-sm font-medium">
            {t('settings.update.newVersionMac', { version })}
          </p>
          <p className="text-xs text-muted-foreground mt-0.5">
            {t('settings.update.newVersionMacHint')}
          </p>
        </div>
        <div className="flex justify-end gap-2">
          <Button
            variant="ghost"
            size="sm"
            className="cursor-pointer"
            onClick={() => {
              toast.dismiss(UPDATE_TOAST_ID);
              onDismiss();
            }}
          >
            {t('settings.update.dismiss')}
          </Button>
          <Button
            size="sm"
            className="cursor-pointer"
            onClick={() => window.open(releaseUrl, '_blank')}
          >
            {t('settings.update.goToDownload')}
          </Button>
        </div>
      </div>
    ),
    { id: UPDATE_TOAST_ID, duration: Infinity }
  );
}
