import { memo, useCallback, useState } from 'react';
import { AlertTriangle, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Label } from '@/components/ui/label';
import { Skeleton } from '@/components/ui/skeleton';
import { useMutationStore } from '@/stores/mutation';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { executeSkillRemoval } from '@/workflows/skill-remove';

export const DeleteSkillDialog = memo(function DeleteSkillDialog() {
  const { t } = useTranslation();
  const target = useSkillDialogStore((state) => state.deleteTarget);
  const preview = useSkillDialogStore((state) => state.deletePreview);
  const loading = useSkillDialogStore((state) => state.loadingAgentDetails);
  const close = useSkillDialogStore((state) => state.closeDelete);
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [selectedEntries, setSelectedEntries] = useState<Set<string>>(
    () => new Set(preview?.physicalEntries.map((entry) => entry.entryId) ?? []),
  );
  const [removeCanonical, setRemoveCanonical] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [previousPreview, setPreviousPreview] = useState(preview);

  if (preview !== previousPreview) {
    setPreviousPreview(preview);
    setSelectedEntries(new Set(preview?.physicalEntries.map((entry) => entry.entryId) ?? []));
    setRemoveCanonical(false);
  }

  const toggleEntry = useCallback((entryId: string) => {
    setSelectedEntries((current) => {
      const next = new Set(current);
      if (next.has(entryId)) next.delete(entryId);
      else next.add(entryId);
      return next;
    });
  }, []);

  const confirm = useCallback(async () => {
    if (!preview) return;
    setRemoving(true);
    try {
      const entryIds = [...selectedEntries];
      const confirmEntityDirectories = preview.physicalEntries.some(
        (entry) => entryIds.includes(entry.entryId) && entry.kind === 'directory',
      );
      await executeSkillRemoval({ removeCanonical, entryIds, confirmEntityDirectories });
    } finally {
      setRemoving(false);
    }
  }, [preview, removeCanonical, selectedEntries]);

  const canConfirm = removeCanonical || selectedEntries.size > 0;

  return (
    <Dialog open={Boolean(target)} onOpenChange={(open) => !open && !removing && close()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('skills.deleteConfirm.title')}</DialogTitle>
          <DialogDescription>
            {t('skills.deleteConfirm.description', { name: target?.skill.name })}
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="space-y-2"><Skeleton className="h-10 w-full" /><Skeleton className="h-10 w-full" /></div>
        ) : (
          <div className="space-y-3">
            <label className="flex items-start gap-2 rounded-md border p-3">
              <Checkbox checked={removeCanonical} onCheckedChange={(value) => setRemoveCanonical(value === true)} />
              <span className="space-y-1">
                <span className="block text-sm font-medium">{t('skills.deleteConfirm.removeCanonical')}</span>
                <span className="block text-xs text-muted-foreground">{t('skills.deleteConfirm.removeCanonicalHint')}</span>
              </span>
            </label>

            {preview?.physicalEntries.map((entry) => (
              <div key={entry.entryId} className="flex items-start gap-2 rounded-md border border-border/60 p-3">
                <Checkbox
                  id={`remove-entry-${entry.entryId}`}
                  checked={selectedEntries.has(entry.entryId)}
                  onCheckedChange={() => toggleEntry(entry.entryId)}
                />
                <Label htmlFor={`remove-entry-${entry.entryId}`} className="min-w-0 flex-1 cursor-pointer">
                  <span className="flex flex-wrap items-center gap-1.5">
                    {entry.owners.map((owner) => (
                      <Badge key={`${entry.entryId}:${owner.logicalTargetId}`} variant="outline">
                        {owner.displayName}
                      </Badge>
                    ))}
                    <Badge variant="secondary">{entry.kind}</Badge>
                  </span>
                  <span className="mt-1 block truncate font-mono text-xs text-muted-foreground">
                    {entry.displayPath.nativePath}
                  </span>
                </Label>
              </div>
            ))}

            {preview?.physicalEntries.some((entry) => (
              selectedEntries.has(entry.entryId) && entry.kind === 'directory'
            )) ? (
              <div className="flex gap-2 rounded-md border border-warning/30 bg-warning/10 p-3 text-sm">
                <AlertTriangle className="h-4 w-4 shrink-0 text-warning" />
                {t('skills.deleteConfirm.directoryWarning')}
              </div>
            ) : null}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={close} disabled={removing}>{t('common.cancel')}</Button>
          <Button
            variant={removeCanonical ? 'destructive' : 'default'}
            onClick={confirm}
            disabled={writeBlocked || removing || loading || !preview || !canConfirm}
          >
            {removing ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            {t('skills.deleteConfirm.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
