import { useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import {
  SourceSkillSelectionPanel,
  type SourceSkillCandidate,
  type SourceSkillSelectionCopy,
} from '@/components/source-discovery/SourceSkillSelectionPanel';
import type { WizardState } from './types';

interface SkillsStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState> | ((prev: WizardState) => Partial<WizardState>)) => void;
}

export function SkillsStep({ state, updateState }: SkillsStepProps) {
  const { t } = useTranslation();
  const candidates = useMemo<SourceSkillCandidate[]>(
    () => state.availableSkills.map((skill) => ({
      candidateId: skill.name,
      name: skill.name,
      description: skill.description,
      groupName: skill.pluginName,
      selectable: true,
    })),
    [state.availableSkills],
  );
  const copy = useMemo<SourceSkillSelectionCopy>(() => ({
    title: t('addSkill.skills.title'),
    selected: (count, total) => t('addSkill.skills.selected', { count, total }),
    searchPlaceholder: t('addSkill.skills.search'),
    selectAll: t('addSkill.skills.selectAll'),
    clear: t('addSkill.skills.clear'),
    empty: t('addSkill.skills.empty'),
    generalGroup: t('skills.pluginGroup.general'),
  }), [t]);
  const handleSelectionChange = useCallback((selectedSkills: string[]) => {
    updateState({ selectedSkills });
  }, [updateState]);

  return (
    <SourceSkillSelectionPanel
      candidates={candidates}
      selectedCandidateIds={state.selectedSkills}
      query={state.skillSearchQuery}
      onQueryChange={(skillSearchQuery) => updateState({ skillSearchQuery })}
      onSelectionChange={handleSelectionChange}
      copy={copy}
    />
  );
}
