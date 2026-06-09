// src/components/skills/ManageAgentsDialog.tsx
import { useState, useCallback, useMemo, useEffect, memo } from 'react';
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
import { isAutomaticAgent } from '@/lib/agentTargets';
import { AgentSelector } from './add-skill/AgentSelector';
import type { InstalledSkill, SkillScope, AgentInfo, InstallMode, SkillAgentDetails } from '@/bindings';

interface ManageAgentsDialogProps {
  skill: InstalledSkill | null;
  scope: SkillScope;
  allAgents: AgentInfo[];
  agentDetails?: SkillAgentDetails | null;
  loadingAgentDetails?: boolean;
  onClose: () => void;
  onSave: (
    addAgents: string[],
    removeAgents: string[],
    mode: InstallMode,
    privateCopyAgents?: string[],
    removePrivateCopyAgents?: string[],
  ) => Promise<void>;
}

export const ManageAgentsDialog = memo(function ManageAgentsDialog({
  skill,
  scope,
  allAgents,
  agentDetails,
  loadingAgentDetails = false,
  onClose,
  onSave,
}: ManageAgentsDialogProps) {
  const initialSelected = useMemo(() => {
    if (!skill) return [] as string[];
    const installScope = scope === 'project' ? 'project' : 'global';
    const automaticIds = new Set(
      allAgents.filter((agent) => isAutomaticAgent(agent, installScope)).map((agent) => agent.id)
    );
    return skill.agents.filter((id) => !automaticIds.has(id));
  }, [skill, allAgents, scope]);

  const resetKey = useMemo(() => {
    const skillKey = skill
      ? `${skill.scope}:${skill.canonicalPath}:${skill.name}`
      : 'none';
    return `${skillKey}:${initialSelected.join('\u001f')}`;
  }, [skill, initialSelected]);

  return (
    <ManageAgentsDialogBody
      key={resetKey}
      skill={skill}
      scope={scope}
      allAgents={allAgents}
      agentDetails={agentDetails}
      loadingAgentDetails={loadingAgentDetails}
      initialSelected={initialSelected}
      onClose={onClose}
      onSave={onSave}
    />
  );
});

interface ManageAgentsDialogBodyProps extends ManageAgentsDialogProps {
  initialSelected: string[];
}

function ManageAgentsDialogBody({
  skill,
  scope,
  allAgents,
  agentDetails,
  loadingAgentDetails = false,
  initialSelected,
  onClose,
  onSave,
}: ManageAgentsDialogBodyProps) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);
  const [mode, setMode] = useState<InstallMode>('symlink');

  const [selectedAgents, setSelectedAgents] = useState<string[]>(initialSelected);
  const [privateCopyAgents, setPrivateCopyAgents] = useState<string[]>([]);
  const [privateCopyAgentsExpanded, setPrivateCopyAgentsExpanded] = useState(false);
  const duplicateAgents = useMemo(
    () => agentDetails?.duplicateCopyAgents ?? [],
    [agentDetails?.duplicateCopyAgents]
  );
  const duplicateAgentIds = useMemo(
    () => new Set<string>(duplicateAgents.map((agent) => agent.agent)),
    [duplicateAgents]
  );
  useEffect(() => {
    setPrivateCopyAgents((current) => {
      const next = [...current];
      for (const agentId of duplicateAgentIds) {
        if (!next.includes(agentId)) next.push(agentId);
      }
      return next;
    });
  }, [duplicateAgentIds]);
  const handlePrivateCopyChange = useCallback((agents: string[]) => {
    setPrivateCopyAgents(agents);
  }, []);

  // 计算 diff
  const { addAgents, removeAgents, addPrivateCopyAgents, removePrivateCopyAgents, hasChanges } = useMemo(() => {
    const initialSet = new Set(initialSelected);
    const currentSet = new Set(selectedAgents);
    const add = selectedAgents.filter((id) => !initialSet.has(id));
    const remove = initialSelected.filter((id) => !currentSet.has(id));
    const newPrivateCopyAgents = privateCopyAgents.filter((id) => !duplicateAgentIds.has(id));
    const removePrivateCopyAgents = [...duplicateAgentIds].filter((id) => !privateCopyAgents.includes(id));
    return {
      addAgents: add,
      removeAgents: remove,
      addPrivateCopyAgents: newPrivateCopyAgents,
      removePrivateCopyAgents,
      hasChanges: add.length > 0 || remove.length > 0 || newPrivateCopyAgents.length > 0 || removePrivateCopyAgents.length > 0,
    };
  }, [selectedAgents, initialSelected, privateCopyAgents, duplicateAgentIds]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      await onSave(addAgents, removeAgents, mode, addPrivateCopyAgents, removePrivateCopyAgents);
    } finally {
      setSaving(false);
    }
  }, [onSave, addAgents, removeAgents, mode, addPrivateCopyAgents, removePrivateCopyAgents]);

  const showMode = addAgents.length > 0;
  const modeDisabled = saving;

  return (
    <Dialog open={!!skill} onOpenChange={(open) => !open && !saving && onClose()}>
      <DialogContent className="min-w-0 max-w-[calc(100vw-2rem)] sm:max-w-xl gap-0 overflow-hidden">
        <DialogHeader className="min-w-0">
          <DialogTitle>{t('skills.manageAgents.title')}</DialogTitle>
          <DialogDescription className="min-w-0 break-words">
            {t('skills.manageAgents.description', { name: skill?.name })}
          </DialogDescription>
        </DialogHeader>

        <div data-testid="manage-agents-dialog-body" className="mt-4 min-w-0 max-w-full max-h-[60vh] overflow-y-auto overflow-x-hidden">
          <AgentSelector
            selectedAgents={selectedAgents}
            privateCopyAgents={privateCopyAgents}
            allAgents={allAgents}
            onSelectionChange={setSelectedAgents}
            onPrivateCopyChange={handlePrivateCopyChange}
            scope={scope === 'project' ? 'project' : 'global'}
            privateCopyAgentsExpanded={privateCopyAgentsExpanded}
            onPrivateCopyExpandedChange={setPrivateCopyAgentsExpanded}
          />
        </div>

        {loadingAgentDetails ? (
          <div className="mt-4 rounded-md border border-border/50 bg-muted/10 px-3 py-2 text-xs text-muted-foreground">
            {t('common.loading')}
          </div>
        ) : null}

        {showMode && (
        <div
          className="mt-4 min-w-0 max-w-full pt-4 border-t border-border/50 space-y-2"
          aria-disabled={modeDisabled}
        >
          <Label className="text-[13px] font-semibold text-foreground">
            {t('skills.manageAgents.modeTitle')}
          </Label>
          <RadioGroup
            value={mode}
            onValueChange={(value) => setMode(value as InstallMode)}
            className={`space-y-1 transition-opacity gap-1 ${modeDisabled ? 'opacity-50 pointer-events-none' : ''}`}
          >
            <Label
              htmlFor="manage-mode-symlink"
              className={`flex items-start gap-3 px-3 py-2 rounded-md cursor-pointer border transition-colors ${
                mode === 'symlink'
                  ? 'bg-accent/50 border-accent/80'
                  : 'border-transparent hover:bg-accent/30'
              }`}
            >
              <RadioGroupItem
                value="symlink"
                id="manage-mode-symlink"
                className="mt-0.5"
                disabled={modeDisabled}
              />
              <div className="space-y-0.5 flex-1 min-w-0">
                <div className="text-[13px] font-medium leading-none">
                  {t('addSkill.mode.symlink')}
                </div>
                <p className="text-[11px] text-muted-foreground/70">
                  {t('skills.manageAgents.symlinkHint')}
                </p>
              </div>
            </Label>
            <Label
              htmlFor="manage-mode-copy"
              className={`flex items-start gap-3 px-3 py-2 rounded-md cursor-pointer border transition-colors ${
                mode === 'copy'
                  ? 'bg-accent/50 border-accent/80'
                  : 'border-transparent hover:bg-accent/30'
              }`}
            >
              <RadioGroupItem
                value="copy"
                id="manage-mode-copy"
                className="mt-0.5"
                disabled={modeDisabled}
              />
              <div className="space-y-0.5 flex-1 min-w-0">
                <div className="text-[13px] font-medium leading-none">
                  {t('addSkill.mode.copy')}
                </div>
                <p className="text-[11px] text-muted-foreground/70">
                  {t('skills.manageAgents.copyHint')}
                </p>
              </div>
            </Label>
          </RadioGroup>
        </div>
        )}

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
}
