// src/components/skills/add-skill/SkillsStep.tsx
import { useMemo, useCallback, memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { toTitleCase, cn } from '@/lib/utils';
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

  // 按 plugin 分组（仅在有 pluginName 的 skills 存在时才分组）— js-combine-iterations
  const groupedSkills = useMemo(() => {
    const groups: Record<string, AvailableSkill[]> = {};
    const ungrouped: AvailableSkill[] = [];
    let hasAnyPlugin = false;

    for (const skill of filteredSkills) {
      if (skill.pluginName) {
        hasAnyPlugin = true;
        if (!groups[skill.pluginName]) groups[skill.pluginName] = [];
        groups[skill.pluginName].push(skill);
      } else {
        ungrouped.push(skill);
      }
    }

    return hasAnyPlugin ? { groups, ungrouped } : null;
  }, [filteredSkills]);

  // O(1) 查找已选 skills — js-index-maps
  const selectedSet = useMemo(
    () => new Set(state.selectedSkills),
    [state.selectedSkills]
  );

  const toggleSkill = useCallback((skillName: string) => {
    updateState((prev) => ({
      selectedSkills: prev.selectedSkills.includes(skillName)
        ? prev.selectedSkills.filter((s) => s !== skillName)
        : [...prev.selectedSkills, skillName],
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
      {/* 头部标题与汇总 — 固定不滚动 */}
      <div className="flex-shrink-0 flex items-center justify-between mb-1">
        <h3 className="text-sm font-medium">{t('addSkill.skills.title')}</h3>
        <span className="text-sm text-muted-foreground font-medium">
          {t('addSkill.skills.selected', {
            count: state.selectedSkills.length,
            total: state.availableSkills.length,
          })}
        </span>
      </div>

      {/* 搜索工具栏 — 极简融合设计 */}
      <div className="flex-shrink-0 flex items-center gap-3 relative mb-3">
        <Input
          value={state.skillSearchQuery}
          onChange={(e) => updateState({ skillSearchQuery: e.target.value })}
          placeholder={t('addSkill.skills.search')}
          className="flex-1 h-11 bg-card/80 backdrop-blur-sm shadow-sm transition-all rounded-xl border-muted-foreground/20"
        />
        <div className="flex items-center gap-1 shrink-0 px-1">
          <Button variant="ghost" size="sm" onClick={selectAll} className="text-muted-foreground hover:text-foreground px-2 h-8">
            {t('addSkill.skills.selectAll')}
          </Button>
          <div className="w-px h-3.5 bg-border mx-1" />
          <Button variant="ghost" size="sm" onClick={clearSelection} className="text-muted-foreground hover:text-foreground px-2 h-8">
            {t('addSkill.skills.clear')}
          </Button>
        </div>
      </div>

      {/* 可用 Skills 列表 — 彻底打碎外框，采用独立的悬浮卡片 */}
      <div className="flex-1 min-h-0 overflow-y-auto pr-2 space-y-3 pb-2">
        {filteredSkills.length === 0 ? (
          <div className="p-8 text-center text-sm text-muted-foreground">
            {t('addSkill.skills.empty')}
          </div>
        ) : groupedSkills ? (
          /* 按 plugin 分组展示 */
          <>
            {Object.keys(groupedSkills.groups).sort().map((groupName) => (
              <div key={groupName} className="space-y-2">
                <div className="px-2 py-1 text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                  {toTitleCase(groupName)}
                </div>
                {groupedSkills.groups[groupName].map((skill) => (
                  <SkillItem
                    key={skill.name}
                    skill={skill}
                    selected={selectedSet.has(skill.name)}
                    onToggle={toggleSkill}
                  />
                ))}
              </div>
            ))}
            {groupedSkills.ungrouped.length > 0 && (
              <div className="space-y-2 pt-2">
                <div className="px-2 py-1 text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                  {t('skills.pluginGroup.general')}
                </div>
                {groupedSkills.ungrouped.map((skill) => (
                  <SkillItem
                    key={skill.name}
                    skill={skill}
                    selected={selectedSet.has(skill.name)}
                    onToggle={toggleSkill}
                  />
                ))}
              </div>
            )}
          </>
        ) : (
          /* 无分组，扁平列表 */
          filteredSkills.map((skill) => (
            <SkillItem
              key={skill.name}
              skill={skill}
              selected={selectedSet.has(skill.name)}
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
      className={cn(
        "flex items-start gap-4 p-4 transition-all cursor-pointer rounded-xl border shadow-sm hover:shadow-md",
        selected
          ? "bg-primary/5 border-primary/30 shadow-primary/5"
          : "bg-card/60 backdrop-blur-sm border-transparent hover:border-border/50 hover:bg-card"
      )}
      onClick={() => onToggle(skill.name)}
    >
      <Checkbox checked={selected} className="mt-0.5 transition-transform shrink-0" />
      <div className="flex-1 min-w-0">
        <div className={cn("font-medium text-sm transition-colors", selected ? "text-primary" : "text-foreground")}>
          {skill.name}
        </div>
        <div className="text-xs text-muted-foreground line-clamp-2 mt-1 leading-relaxed">
          {skill.description}
        </div>
      </div>
    </div>
  );
});
