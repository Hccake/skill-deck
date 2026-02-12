// src/components/skills/add-skill/SkillsStep.tsx
import { useMemo, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import type { AvailableSkill } from '@/bindings';
import type { AddSkillState } from './types';

interface SkillsStepProps {
  state: AddSkillState;
  updateState: (updates: Partial<AddSkillState> | ((prev: AddSkillState) => Partial<AddSkillState>)) => void;
}

export function SkillsStep({ state, updateState }: SkillsStepProps) {
  const { t } = useTranslation();

  // Filter skills by search query
  const filteredSkills = useMemo(() => {
    const query = state.skillSearchQuery.toLowerCase();
    if (!query) return state.availableSkills;

    return state.availableSkills.filter(
      (skill) =>
        skill.name.toLowerCase().includes(query) ||
        skill.description.toLowerCase().includes(query)
    );
  }, [state.availableSkills, state.skillSearchQuery]);

  // Toggle skill selection - 使用函数式 setState，符合 rerender-functional-setstate 规则
  const toggleSkill = useCallback((skillName: string) => {
    updateState((prev) => ({
      selectedSkills: prev.selectedSkills.includes(skillName)
        ? prev.selectedSkills.filter((s) => s !== skillName)
        : [...prev.selectedSkills, skillName],
    }));
  }, [updateState]); // 稳定依赖

  // Select all - 使用函数式 setState
  const selectAll = useCallback(() => {
    updateState((prev) => ({
      selectedSkills: prev.availableSkills.map((s) => s.name),
    }));
  }, [updateState]);

  const clearSelection = useCallback(() => {
    updateState({ selectedSkills: [] });
  }, [updateState]);

  return (
    <div className="space-y-4 py-1">
      {/* Header with count / validation */}
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">{t('addSkill.skills.title')}</h3>
        {state.selectedSkills.length === 0 ? (
          <span className="text-sm text-destructive">
            {t('addSkill.skills.required')}
          </span>
        ) : (
          <span className="text-sm text-muted-foreground">
            {t('addSkill.skills.selected', {
              count: state.selectedSkills.length,
              total: state.availableSkills.length,
            })}
          </span>
        )}
      </div>

      {/* Search */}
      <Input
        value={state.skillSearchQuery}
        onChange={(e) => updateState({ skillSearchQuery: e.target.value })}
        placeholder={t('addSkill.skills.search')}
      />

      {/* Skills list */}
      <div className="border rounded-md p-2 space-y-1">
          {filteredSkills.length === 0 ? (
            <div className="p-4 text-center text-sm text-muted-foreground">
              {t('addSkill.skills.empty')}
            </div>
          ) : (
            filteredSkills.map((skill) => (
              <SkillItem
                key={skill.name}
                skill={skill}
                selected={state.selectedSkills.includes(skill.name)}
                onToggle={() => toggleSkill(skill.name)}
              />
            ))
          )}
      </div>

      {/* Actions */}
      <div className="flex gap-2">
        <Button variant="outline" size="sm" onClick={selectAll}>
          {t('addSkill.skills.selectAll')}
        </Button>
        <Button variant="outline" size="sm" onClick={clearSelection}>
          {t('addSkill.skills.clear')}
        </Button>
      </div>
    </div>
  );
}

// 使用 memo 包装，符合 rerender-memo 规则
const SkillItem = memo(function SkillItem({
  skill,
  selected,
  onToggle,
}: {
  skill: AvailableSkill;
  selected: boolean;
  onToggle: () => void;
}) {
  return (
    <div
      className="flex items-start gap-3 p-2 rounded-md hover:bg-muted/50 cursor-pointer"
      onClick={onToggle}
    >
      <Checkbox checked={selected} className="mt-1" />
      <div className="flex-1 min-w-0">
        <div className="font-medium text-sm">{skill.name}</div>
        <div className="text-xs text-muted-foreground line-clamp-2">
          {skill.description}
        </div>
      </div>
    </div>
  );
});
