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
  ) => Promise<void>;
  onCleanupDuplicates?: (agents: string[]) => Promise<void>;
}

export const ManageAgentsDialog = memo(function ManageAgentsDialog({
  skill,
  scope,
  allAgents,
  agentDetails,
  loadingAgentDetails = false,
  onClose,
  onSave,
  onCleanupDuplicates,
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
      onCleanupDuplicates={onCleanupDuplicates}
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
  onCleanupDuplicates,
}: ManageAgentsDialogBodyProps) {
  const { t } = useTranslation();
  const [saving, setSaving] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [cleanupConfirmOpen, setCleanupConfirmOpen] = useState(false);
  const [mode, setMode] = useState<InstallMode>('symlink');

  const [selectedAgents, setSelectedAgents] = useState<string[]>(initialSelected);
  const [privateCopyAgents, setPrivateCopyAgents] = useState<string[]>([]);
  const [privateCopyAgentsExpanded, setPrivateCopyAgentsExpanded] = useState(false);
  const duplicateAgents = useMemo(
    () => agentDetails?.duplicateCopyAgents ?? [],
    [agentDetails?.duplicateCopyAgents]
  );
  const privateOnlyAgents = useMemo(
    () => agentDetails?.privateOnlyAgents ?? [],
    [agentDetails?.privateOnlyAgents]
  );
  const duplicateAgentIds = useMemo(
    () => new Set<string>(duplicateAgents.map((agent) => agent.agent)),
    [duplicateAgents]
  );
  useEffect(() => {
    if (duplicateAgentIds.size === 0) return;
    setPrivateCopyAgents((current) => current.filter((agent) => !duplicateAgentIds.has(agent)));
  }, [duplicateAgentIds]);
  const handlePrivateCopyChange = useCallback((agents: string[]) => {
    setPrivateCopyAgents(agents.filter((agent) => !duplicateAgentIds.has(agent)));
  }, [duplicateAgentIds]);

  // 计算 diff
  const { addAgents, removeAgents, hasChanges } = useMemo(() => {
    const initialSet = new Set(initialSelected);
    const currentSet = new Set(selectedAgents);
    const add = selectedAgents.filter((id) => !initialSet.has(id));
    const remove = initialSelected.filter((id) => !currentSet.has(id));
    return {
      addAgents: add,
      removeAgents: remove,
      hasChanges: add.length > 0 || remove.length > 0 || privateCopyAgents.length > 0,
    };
  }, [selectedAgents, initialSelected, privateCopyAgents]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      await onSave(addAgents, removeAgents, mode, privateCopyAgents);
    } finally {
      setSaving(false);
    }
  }, [onSave, addAgents, removeAgents, mode, privateCopyAgents]);

  const handleCleanAgents = useCallback(async (agents: string[]) => {
    if (!onCleanupDuplicates || agents.length === 0) return;
    setCleaning(true);
    try {
      await onCleanupDuplicates(agents);
    } finally {
      setCleaning(false);
      setCleanupConfirmOpen(false);
    }
  }, [onCleanupDuplicates]);

  const handleCleanAll = useCallback(() => {
    if (!onCleanupDuplicates || duplicateAgents.length === 0) return;
    setCleanupConfirmOpen(true);
  }, [duplicateAgents.length, onCleanupDuplicates]);

  const handleConfirmCleanAll = useCallback(async () => {
    await handleCleanAgents(duplicateAgents.map((agent) => agent.agent));
  }, [duplicateAgents, handleCleanAgents]);

  const showMode = addAgents.length > 0;
  const modeDisabled = saving || cleaning;

  return (
    <>
    <Dialog open={!!skill} onOpenChange={(open) => !open && !saving && !cleaning && onClose()}>
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
            privateCopyAgents={privateCopyAgents}
            excludedPrivateCopyAgents={[...duplicateAgentIds]}
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
        ) : duplicateAgents.length > 0 ? (
          <div className="mt-4 rounded-md border border-amber-500/30 bg-amber-500/5 px-3 py-3">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 space-y-1">
                <div className="text-[13px] font-semibold text-foreground">
                  {t('skills.manageAgents.duplicateCopiesTitle')}
                </div>
                <p className="text-xs leading-5 text-muted-foreground">
                  {t('skills.manageAgents.duplicateCopiesHint')}
                </p>
                <div className="flex flex-col gap-1.5 pt-1">
                  {duplicateAgents.map((agent) => (
                    <div key={agent.agent} className="flex items-center justify-between gap-2 rounded border border-border/60 bg-background/70 px-2 py-1.5">
                      <div className="min-w-0">
                        <div className="truncate text-[12px] font-medium text-foreground">{agent.displayName}</div>
                        {agent.privatePath ? (
                          <code className="block truncate font-mono text-[10px] leading-4 text-muted-foreground/80">
                            {agent.privatePath}
                          </code>
                        ) : null}
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 shrink-0 px-2 text-[11px]"
                        disabled={cleaning || saving || !onCleanupDuplicates}
                        onClick={() => handleCleanAgents([agent.agent])}
                      >
                        {t('skills.manageAgents.cleanDuplicateCopy')}
                      </Button>
                    </div>
                  ))}
                </div>
              </div>
              <Button
                variant="outline"
                size="sm"
                className="shrink-0"
                disabled={cleaning || saving || !onCleanupDuplicates}
                onClick={handleCleanAll}
              >
                {cleaning ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
                {t('skills.manageAgents.cleanAllDuplicates')}
              </Button>
            </div>
          </div>
        ) : null}

        {privateOnlyAgents.length > 0 ? (
          <div className="mt-4 rounded-md border border-border/60 bg-muted/10 px-3 py-3">
            <div className="space-y-1">
              <div className="text-[13px] font-semibold text-foreground">
                {t('skills.manageAgents.privateOnlyTitle')}
              </div>
              <p className="text-xs leading-5 text-muted-foreground">
                {t('skills.manageAgents.privateOnlyHint')}
              </p>
            </div>
            <div className="mt-2 flex flex-col gap-1.5">
              {privateOnlyAgents.map((agent) => (
                <div key={agent.agent} className="min-w-0 rounded border border-border/60 bg-background/70 px-2 py-1.5">
                  <div className="truncate text-[12px] font-medium text-foreground">{agent.displayName}</div>
                  {agent.privatePath ? (
                    <code className="block truncate font-mono text-[10px] leading-4 text-muted-foreground/80">
                      {agent.privatePath}
                    </code>
                  ) : null}
                </div>
              ))}
            </div>
          </div>
        ) : null}

        {showMode && (
        <div
          className="mt-4 pt-4 border-t border-border/50 space-y-2"
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
          <Button variant="outline" onClick={onClose} disabled={saving || cleaning}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={handleSave}
            disabled={saving || cleaning || !hasChanges}
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
    <AlertDialog open={cleanupConfirmOpen} onOpenChange={(open) => !cleaning && setCleanupConfirmOpen(open)}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('skills.manageAgents.cleanupConfirmTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('skills.manageAgents.cleanupConfirmDescription', { count: duplicateAgents.length })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="max-h-48 space-y-1 overflow-y-auto rounded-md border border-border/60 bg-muted/15 p-2">
          {duplicateAgents.map((agent) => (
            <div key={agent.agent} className="min-w-0 text-xs">
              <div className="font-medium text-foreground">{agent.displayName}</div>
              {agent.privatePath ? (
                <code className="block truncate font-mono text-[11px] text-muted-foreground">
                  {agent.privatePath}
                </code>
              ) : null}
            </div>
          ))}
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={cleaning}>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction
            disabled={cleaning}
            onClick={(event) => {
              event.preventDefault();
              void handleConfirmCleanAll();
            }}
          >
            {cleaning ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : null}
            {t('skills.manageAgents.confirmCleanupDuplicates')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
    </>
  );
}
