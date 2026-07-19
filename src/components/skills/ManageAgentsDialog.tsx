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
import { agentId } from '@/lib/agents';
import { canCreatePrivateCopy, isPrivateRequiredAgent } from '@/lib/agentTargets';
import { AgentSelector } from '@/components/agents/AgentSelector';
import type {
  AgentId,
  AgentSelectionGroup,
  InstalledSkill,
  SkillScope,
  ResolvedAgent,
  InstallMode,
  ManageAgentsPreview,
  ObservedEntryId,
  ObservedPhysicalEntry,
} from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

interface ManageAgentsDialogProps {
  skill: InstalledSkill | null;
  scope: SkillScope;
  allAgents: ResolvedAgent[];
  agentDetails?: ManageAgentsPreview | null;
  loadingAgentDetails?: boolean;
  onClose: () => void;
  onSave: (
    addAgents: AgentId[],
    removeEntryIds: ObservedEntryId[],
    mode: InstallMode,
    addOptionalAgents: AgentId[],
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
    if (!skill) return [] as AgentId[];
    const installScope = scope === 'project' ? 'project' : 'global';
    const agentById = new Map(allAgents.map((agent) => [agentId(agent), agent]));
    const observedIds = agentDetails?.observedEntries.flatMap((entry) =>
      entry.owners.map((owner) => owner.agentId)) ?? [];
    const sourceIds = observedIds.length > 0
      ? observedIds
      : (skill.privateAdaptedAgents ?? skill.agents);
    return [...new Set(sourceIds)].filter((id) => {
      const agent = agentById.get(id);
      return !agent || isPrivateRequiredAgent(agent, installScope);
    });
  }, [agentDetails?.observedEntries, allAgents, scope, skill]);
  const initialOptional = useMemo(() => {
    const installScope = scope === 'project' ? 'project' : 'global';
    const agentById = new Map(allAgents.map((agent) => [agentId(agent), agent]));
    return [...new Set(agentDetails?.observedEntries.flatMap((entry) =>
      entry.owners.map((owner) => owner.agentId)) ?? [])]
      .filter((id) => {
        const agent = agentById.get(id);
        return agent ? canCreatePrivateCopy(agent, installScope) : false;
      });
  }, [agentDetails?.observedEntries, allAgents, scope]);
  const resetKey = useMemo(() => {
    const skillKey = skill
      ? `${skill.scope}:${skill.canonicalPath}:${skill.name}`
      : 'none';
    const entriesKey = agentDetails?.observedEntries.map((entry) => entry.entryId).join('\u001f') ?? '';
    return `${skillKey}:${initialSelected.join('\u001f')}:${initialOptional.join('\u001f')}:${entriesKey}`;
  }, [agentDetails?.observedEntries, initialOptional, initialSelected, skill]);

  return (
    <ManageAgentsDialogBody
      key={resetKey}
      skill={skill}
      scope={scope}
      allAgents={allAgents}
      agentDetails={agentDetails}
      loadingAgentDetails={loadingAgentDetails}
      initialSelected={initialSelected}
      initialOptional={initialOptional}
      onClose={onClose}
      onSave={onSave}
    />
  );
});

interface ManageAgentsDialogBodyProps extends ManageAgentsDialogProps {
  initialSelected: AgentId[];
  initialOptional: AgentId[];
}

function ManageAgentsDialogBody({
  skill,
  scope,
  allAgents,
  agentDetails,
  loadingAgentDetails = false,
  initialSelected,
  initialOptional,
  onClose,
  onSave,
}: ManageAgentsDialogBodyProps) {
  const { t } = useTranslation();
  const writeBlocked = useMutationStore((state) => state.activeMutation !== null);
  const [saving, setSaving] = useState(false);
  const [mode, setMode] = useState<InstallMode>('symlink');
  const selectableAgents = useMemo(
    () => agentDetails?.availableAgents ?? allAgents,
    [agentDetails?.availableAgents, allAgents],
  );
  const installScope = scope === 'project' ? 'project' : 'global';
  const selectionGroups = useMemo(
    () => mergeObservedSelectionGroups(
      agentDetails?.selectionGroups?.[installScope] ?? [],
      agentDetails?.observedEntries ?? [],
    ),
    [agentDetails?.observedEntries, agentDetails?.selectionGroups, installScope],
  );
  const mixedSelectionGroups = useMemo(() => {
    const requiredIds = new Set(selectableAgents
      .filter((agent) => isPrivateRequiredAgent(agent, installScope))
      .map(agentId));
    const optionalIds = new Set(selectableAgents
      .filter((agent) => canCreatePrivateCopy(agent, installScope))
      .map(agentId));
    return selectionGroups.flatMap((group) => {
      const required = group.agentIds.filter((id) => requiredIds.has(id));
      const optional = group.agentIds.filter((id) => optionalIds.has(id));
      return required.length > 0 && optional.length > 0 ? [{ required, optional }] : [];
    });
  }, [installScope, selectableAgents, selectionGroups]);
  const normalizedInitialSelection = useMemo(() => {
    const required = new Set(initialSelected);
    const optional = new Set(initialOptional);
    for (const group of mixedSelectionGroups) {
      if (!group.required.some((id) => required.has(id))
        && !group.optional.some((id) => optional.has(id))) continue;
      group.required.forEach((id) => required.add(id));
      group.optional.forEach((id) => optional.add(id));
    }
    return { required: [...required], optional: [...optional] };
  }, [initialOptional, initialSelected, mixedSelectionGroups]);
  const [selectedAgents, setSelectedAgents] = useState<AgentId[]>(normalizedInitialSelection.required);
  const [optionalAgents, setOptionalAgents] = useState<AgentId[]>(normalizedInitialSelection.optional);
  const [optionalExpanded, setOptionalExpanded] = useState(normalizedInitialSelection.optional.length > 0);
  const { addAgents, addOptionalAgents, removeEntryIds, hasChanges } = useMemo(() => {
    const initialSet = new Set(normalizedInitialSelection.required);
    const initialOptionalSet = new Set(normalizedInitialSelection.optional);
    const currentSet = new Set(selectedAgents);
    const currentOptionalSet = new Set(optionalAgents);
    const add = selectedAgents.filter((id) => !initialSet.has(id));
    const addOptional = optionalAgents.filter((id) => !initialOptionalSet.has(id));
    const selectedIds = new Set([...currentSet, ...currentOptionalSet]);
    const availableIds = new Set(selectableAgents.map(agentId));
    const removeEntries = (agentDetails?.observedEntries ?? [])
      .filter((entry) => entry.owners.length > 0 && entry.owners.every((owner) =>
        availableIds.has(owner.agentId) && !selectedIds.has(owner.agentId)))
      .map((entry) => entry.entryId);
    return {
      addAgents: add,
      addOptionalAgents: addOptional,
      removeEntryIds: removeEntries,
      hasChanges: add.length > 0 || addOptional.length > 0 || removeEntries.length > 0,
    };
  }, [agentDetails?.observedEntries, normalizedInitialSelection, optionalAgents, selectableAgents, selectedAgents]);

  const handleRequiredSelectionChange = useCallback((nextRequired: AgentId[]) => {
    const previousRequiredIds = new Set(selectedAgents);
    const nextRequiredIds = new Set(nextRequired);
    setSelectedAgents(nextRequired);
    setOptionalAgents((currentOptional) => {
      const nextOptional = new Set(currentOptional);
      for (const group of mixedSelectionGroups) {
        const wasSelected = group.required.some((id) => previousRequiredIds.has(id));
        const isSelected = group.required.some((id) => nextRequiredIds.has(id));
        if (wasSelected === isSelected) continue;
        group.optional.forEach((id) => {
          if (isSelected) nextOptional.add(id);
          else nextOptional.delete(id);
        });
      }
      return [...nextOptional];
    });
  }, [mixedSelectionGroups, selectedAgents]);

  const handleSave = useCallback(async () => {
    setSaving(true);
    try {
      await onSave(addAgents, removeEntryIds, mode, addOptionalAgents);
    } finally {
      setSaving(false);
    }
  }, [addAgents, addOptionalAgents, mode, onSave, removeEntryIds]);

  const showMode = addAgents.length > 0 || addOptionalAgents.length > 0;
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
            privateCopyAgents={optionalAgents}
            allAgents={selectableAgents}
            selectionGroups={selectionGroups}
            onSelectionChange={handleRequiredSelectionChange}
            onPrivateCopyChange={setOptionalAgents}
            scope={scope === 'project' ? 'project' : 'global'}
            privateCopyAgentsExpanded={optionalExpanded}
            onPrivateCopyExpandedChange={setOptionalExpanded}
            showPaths={false}
          />
        </div>

        {loadingAgentDetails ? (
          <div role="status" aria-live="polite" className="mt-4 rounded-md border border-border/50 bg-muted/10 px-3 py-2 text-xs text-muted-foreground">
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
            disabled={writeBlocked || saving || !hasChanges}
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

function mergeObservedSelectionGroups(
  backendGroups: AgentSelectionGroup[],
  observedEntries: ObservedPhysicalEntry[],
): AgentSelectionGroup[] {
  const parent = new Map<AgentId, AgentId>();

  const find = (id: AgentId): AgentId => {
    const current = parent.get(id);
    if (!current) {
      parent.set(id, id);
      return id;
    }
    if (current === id) return id;
    const root = find(current);
    parent.set(id, root);
    return root;
  };
  const union = (ids: AgentId[]) => {
    if (ids.length === 0) return;
    const root = find(ids[0]);
    for (const id of ids.slice(1)) parent.set(find(id), root);
  };

  for (const group of backendGroups) union(group.agentIds);
  for (const entry of observedEntries) union(entry.owners.map((owner) => owner.agentId));

  const idsByRoot = new Map<AgentId, AgentId[]>();
  for (const id of parent.keys()) {
    const root = find(id);
    const ids = idsByRoot.get(root) ?? [];
    ids.push(id);
    idsByRoot.set(root, ids);
  }

  return [...idsByRoot.values()].map((ids) => {
    const agentIds = [...new Set(ids)].sort();
    return {
      groupId: `manage:${agentIds.join(':')}`,
      agentIds,
    };
  });
}
