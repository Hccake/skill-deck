// src/components/skills/add-skill/SkillsStep.tsx
import { useMemo, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import type { AvailableSkill } from '@/bindings';
import type { WizardState } from './types';

interface SkillsStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState> | ((prev: WizardState) => Partial<WizardState>)) => void;
}

export function SkillsStep({ state, updateState }: SkillsStepProps) {
  const { t } = useTranslation();

  const filteredSkills = useMemo(() => {
    const query = state.skillSearchQuery.toLowerCase();
    if (!query) return state.availableSkills;
    return state.availableSkills.filter(
      (skill) =>
        skill.name.toLowerCase().includes(query) ||
        skill.description.toLowerCase().includes(query)
    );
  }, [state.availableSkills, state.skillSearchQuery]);

  const toggleSkill = useCallback((skillName: string) => {
    updateState((prev) => ({
      selectedSkills: prev.selectedSkills.includes(skillName)
        ? prev.selectedSkills.filter((s) => s !== skillName)
        : [...prev.selectedSkills, skillName],
    }));
  }, [updateState]);

  const removeSkill = useCallback((skillName: string) => {
    updateState((prev) => ({
      selectedSkills: prev.selectedSkills.filter((s) => s !== skillName),
    }));
  }, [updateState]);

  const selectAll = useCallback(() => {
    updateState((prev) => ({
      selectedSkills: prev.availableSkills.map((s) => s.name),
    }));
  }, [updateState]);

  const clearSelection = useCallback(() => {
    updateState({ selectedSkills: [] });
  }, [updateState]);

  return (
    <div className="flex flex-col h-full gap-3">
      {/* 已选 Skills 区域 — 固定不滚动 */}
      <div className="flex-shrink-0">
        <div className="flex items-center justify-between mb-2">
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

        {/* 已选 chips */}
        <div className="min-h-[32px] flex flex-wrap gap-1.5 p-2 bg-muted/30 rounded-md border border-dashed">
          {state.selectedSkills.length === 0 ? (
            <span className="text-xs text-muted-foreground py-0.5">
              {t('addSkill.skills.required')}
            </span>
          ) : (
            state.selectedSkills.map((name) => (
              <Badge
                key={name}
                variant="secondary"
                className="gap-1 cursor-pointer hover:bg-destructive/10"
                onClick={() => removeSkill(name)}
              >
                {name}
                <X className="w-3 h-3" />
              </Badge>
            ))
          )}
        </div>
      </div>

      {/* 搜索工具栏 — 固定不滚动 */}
      <div className="flex-shrink-0 flex items-center gap-2">
        <Input
          value={state.skillSearchQuery}
          onChange={(e) => updateState({ skillSearchQuery: e.target.value })}
          placeholder={t('addSkill.skills.search')}
          className="flex-1"
        />
        <Button variant="outline" size="sm" onClick={selectAll}>
          {t('addSkill.skills.selectAll')}
        </Button>
        <Button variant="outline" size="sm" onClick={clearSelection}>
          {t('addSkill.skills.clear')}
        </Button>
      </div>

      {/* 可用 Skills 列表 — 独立滚动 */}
      <div className="flex-1 min-h-0 overflow-y-auto border rounded-md p-2 space-y-1">
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
              onToggle={toggleSkill}
            />
          ))
        )}
      </div>
    </div>
  );
}

const SkillItem = memo(function SkillItem({
  skill,
  selected,
  onToggle,
}: {
  skill: AvailableSkill;
  selected: boolean;
  onToggle: (skillName: string) => void;
}) {
  return (
    <div
      className="flex items-start gap-3 p-2 rounded-md hover:bg-muted/50 cursor-pointer"
      onClick={() => onToggle(skill.name)}
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
