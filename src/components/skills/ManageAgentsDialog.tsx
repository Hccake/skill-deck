// src/components/skills/ManageAgentsDialog.tsx
import { useState, useCallback, useMemo, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { AgentSelector } from './add-skill/AgentSelector';
import type { InstalledSkill, SkillScope, AgentInfo, InstallMode } from '@/bindings';

interface ManageAgentsDialogProps {
  skill: InstalledSkill | null;
  scope: SkillScope;
  allAgents: AgentInfo[];
  onClose: () => void;
  onSave: (addAgents: string[], removeAgents: string[], mode: InstallMode) => Promise<void>;
}

export const ManageAgentsDialog = memo(function ManageAgentsDialog({
  skill,
  scope,
  allAgents,
  onClose,
  onSave,
}: ManageAgentsDialogProps) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);
  const [mode, setMode] = useState<InstallMode>('symlink');

  // 初始选中的 non-universal agents（从 skill.agents 中过滤掉 universal）
  const initialSelected = useMemo(() => {
    if (!skill) return [] as string[];
    const universalIds = new Set(
      allAgents.filter((a) => a.isUniversal).map((a) => a.id)
    );
    return skill.agents.filter((id) => !universalIds.has(id));
  }, [skill, allAgents]);

  const [selectedAgents, setSelectedAgents] = useState<string[]>(initialSelected);

  // render-time reset: skill 变化时重置
  const [prevSkill, setPrevSkill] = useState(skill);
  if (skill !== prevSkill) {
    setPrevSkill(skill);
    setMode('symlink');
    const universalIds = new Set(
      allAgents.filter((a) => a.isUniversal).map((a) => a.id)
    );
    setSelectedAgents(skill ? skill.agents.filter((id) => !universalIds.has(id)) : []);
  }

  // 计算 diff
  const { addAgents, removeAgents, hasChanges } = useMemo(() => {
    const initialSet = new Set(initialSelected);
    const currentSet = new Set(selectedAgents);
    const add = selectedAgents.filter((id) => !initialSet.has(id));
    const remove = initialSelected.filter((id) => !currentSet.has(id));
    return { addAgents: add, removeAgents: remove, hasChanges: add.length > 0 || remove.length > 0 };
  }, [selectedAgents, initialSelected]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      await onSave(addAgents, removeAgents, mode);
    } finally {
      setSaving(false);
    }
  }, [onSave, addAgents, removeAgents, mode]);

  return (
    <Dialog open={!!skill} onOpenChange={(open) => !open && !saving && onClose()}>
      <DialogContent className="sm:max-w-lg gap-0">
        <DialogHeader>
          <DialogTitle>{t('skills.manageAgents.title')}</DialogTitle>
          <DialogDescription>
            {t('skills.manageAgents.description', { name: skill?.name })}
          </DialogDescription>
        </DialogHeader>

        <div className="mt-4 max-h-[60vh] overflow-y-auto">
          <AgentSelector
            selectedAgents={selectedAgents}
            allAgents={allAgents}
            onSelectionChange={setSelectedAgents}
            scope={scope === 'project' ? 'project' : 'global'}
          />
        </div>

        <div className="mt-4 space-y-3">
          <Label className="text-sm font-medium">
            {t('skills.manageAgents.modeTitle')}
          </Label>
          <RadioGroup
            value={mode}
            onValueChange={(value) => setMode(value as InstallMode)}
            className="space-y-2"
          >
            <div className="flex items-start gap-3">
              <RadioGroupItem
                value="symlink"
                id="manage-mode-symlink"
                className="mt-1"
                disabled={addAgents.length === 0 || saving}
              />
              <div>
                <Label htmlFor="manage-mode-symlink" className="font-medium">
                  {t('addSkill.mode.symlink')}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t('skills.manageAgents.symlinkHint')}
                </p>
              </div>
            </div>
            <div className="flex items-start gap-3">
              <RadioGroupItem
                value="copy"
                id="manage-mode-copy"
                className="mt-1"
                disabled={addAgents.length === 0 || saving}
              />
              <div>
                <Label htmlFor="manage-mode-copy" className="font-medium">
                  {t('addSkill.mode.copy')}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t('skills.manageAgents.copyHint')}
                </p>
              </div>
            </div>
          </RadioGroup>
        </div>

        <DialogFooter className="mt-4">
          <Button variant="outline" onClick={onClose} disabled={saving}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={handleSave}
            disabled={saving || !hasChanges}
          >
            {saving ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t('common.loading')}
              </>
            ) : (
              t('skills.manageAgents.save')
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
});
