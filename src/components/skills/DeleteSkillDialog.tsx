// src/components/skills/DeleteSkillDialog.tsx
import { useState, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { useSkillsStore } from '@/stores/skills';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';

export const DeleteSkillDialog = memo(function DeleteSkillDialog() {
  const { t } = useTranslation();
  const target = useSkillsStore((s) => s.deleteTarget);
  const closeDelete = useSkillsStore((s) => s.closeDelete);
  const deleteSkillAction = useSkillsStore((s) => s.deleteSkill);

  const [isDeleting, setIsDeleting] = useState(false);

  const handleConfirm = useCallback(async () => {
    setIsDeleting(true);
    try {
      await deleteSkillAction();
    } finally {
      setIsDeleting(false);
    }
  }, [deleteSkillAction]);

  return (
    <AlertDialog open={!!target} onOpenChange={(open) => !open && !isDeleting && closeDelete()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('skills.deleteConfirm.title')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('skills.deleteConfirm.description', { name: target?.name })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isDeleting}>
            {t('skills.deleteConfirm.cancel')}
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={(e) => {
              e.preventDefault();
              handleConfirm();
            }}
            disabled={isDeleting}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {isDeleting ? t('common.loading') : t('skills.deleteConfirm.confirm')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
});
