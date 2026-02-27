// src/components/skills/add-skill/ConfirmStep.tsx
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { checkOverwrites, checkSkillAudit } from '@/hooks/useTauriApi';
import type { SkillAuditData } from '@/hooks/useTauriApi';
import { RiskBadge } from '../RiskBadge';
import type { WizardState } from './types';

/** kebab-case → Title Case */
function toTitleCase(kebab: string): string {
  return kebab.split('-').map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(' ');
}

interface ConfirmStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
  scope: 'global' | 'project';
  projectPath?: string;
}

export function ConfirmStep({ state, updateState, scope, projectPath }: ConfirmStepProps) {
  const { t } = useTranslation();

  const updateStateRef = useRef(updateState);
  useEffect(() => { updateStateRef.current = updateState; });

  // 审计数据（组件级 state，不影响 wizard 流程）
  const [auditData, setAuditData] = useState<Partial<Record<string, SkillAuditData>>>({});

  // 并行检测覆盖 + 获取审计数据
  useEffect(() => {
    if (state.selectedAgents.length === 0 || state.selectedSkills.length === 0) return;

    updateStateRef.current({ confirmReady: false });

    const overwritePromise = checkOverwrites(
      state.selectedSkills,
      state.selectedAgents,
      scope,
      scope === 'project' ? projectPath : undefined
    );

    const auditPromise = state.source
      ? checkSkillAudit(state.source, state.selectedSkills).catch(() => null)
      : Promise.resolve(null);

    Promise.all([overwritePromise, auditPromise]).then(([overwriteResult, auditResult]) => {
      const overwrites: Record<string, string[]> = {};
      for (const [key, value] of Object.entries(overwriteResult)) {
        if (value) overwrites[key] = value;
      }
      updateStateRef.current({ overwrites, confirmReady: true });

      if (auditResult) {
        setAuditData(auditResult);
      }
    }).catch((error) => {
      console.error('Failed to check overwrites/audit:', error);
      updateStateRef.current({ overwrites: {}, confirmReady: true });
    });
  }, [state.selectedSkills, state.selectedAgents, state.source, scope, projectPath]);

  // 覆盖统计
  const overwriteCount = useMemo(
    () => Object.values(state.overwrites).filter((agents) => agents.length > 0).length,
    [state.overwrites]
  );

  // 按 plugin 分组选中的 skills
  const groupedSelectedSkills = useMemo(() => {
    const skillMap = new Map(state.availableSkills.map((s) => [s.name, s]));
    const hasAnyPlugin = state.selectedSkills.some((name) => skillMap.get(name)?.pluginName);
    if (!hasAnyPlugin) return null;

    const groups: Record<string, string[]> = {};
    const ungrouped: string[] = [];

    for (const name of state.selectedSkills) {
      const pluginName = skillMap.get(name)?.pluginName;
      if (pluginName) {
        if (!groups[pluginName]) groups[pluginName] = [];
        groups[pluginName].push(name);
      } else {
        ungrouped.push(name);
      }
    }
    return { groups, ungrouped };
  }, [state.selectedSkills, state.availableSkills]);

  // 已选的非 universal agents 信息（用于目录列表）
  const selectedNonUniversalAgents = useMemo(() => {
    const selectedSet = new Set(state.selectedAgents);
    return state.allAgents.filter((a) => selectedSet.has(a.id) && !a.isUniversal);
  }, [state.selectedAgents, state.allAgents]);

  const universalDir = scope === 'global' ? '~/.agents/skills/' : '.agents/skills/';

  const renderSkillRow = (skillName: string) => {
    const overwriteAgents = state.overwrites[skillName] ?? [];
    const hasOverwrite = overwriteAgents.length > 0;
    return (
      <div key={skillName} className="flex items-center justify-between gap-2 px-3 py-2">
        <div className="flex items-center gap-1.5 min-w-0">
          {hasOverwrite && (
            <Tooltip>
              <TooltipTrigger asChild>
                <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
              </TooltipTrigger>
              <TooltipContent>
                {t('addSkill.confirm.willOverwrite', { agents: overwriteAgents.join(', ') })}
              </TooltipContent>
            </Tooltip>
          )}
          <span className="font-mono text-[13px] text-foreground truncate">
            {skillName}
          </span>
        </div>
        {auditData[skillName] && (
          <RiskBadge risk={auditData[skillName].risk} />
        )}
      </div>
    );
  };

  return (
    <div className="space-y-4">
      {/* 集中覆盖警告条 */}
      {state.confirmReady && overwriteCount > 0 && (
        <div className="flex items-center gap-2 px-3 py-2 bg-amber-500/10 border border-amber-500/30 rounded-md text-sm text-amber-700 dark:text-amber-400">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          <span>{t('addSkill.confirm.overwriteCount', { count: overwriteCount })}</span>
        </div>
      )}

      {/* Skills 列表 */}
      <div className="border rounded-md divide-y divide-border/50">
        {!state.confirmReady ? (
          // 统一骨架屏
          state.selectedSkills.map((_, idx) => (
            <div key={idx} className="flex items-center justify-between gap-2 px-3 py-2">
              <Skeleton className="h-4 w-32" />
              <Skeleton className="h-5 w-14 rounded-full" />
            </div>
          ))
        ) : groupedSelectedSkills ? (
          <>
            {Object.keys(groupedSelectedSkills.groups).sort().map((groupName) => (
              <div key={groupName}>
                <div className="px-3 py-1.5 text-xs font-semibold text-muted-foreground uppercase tracking-wider bg-muted/30">
                  {toTitleCase(groupName)}
                </div>
                {groupedSelectedSkills.groups[groupName].map(renderSkillRow)}
              </div>
            ))}
            {groupedSelectedSkills.ungrouped.length > 0 && (
              <div>
                <div className="px-3 py-1.5 text-xs font-semibold text-muted-foreground uppercase tracking-wider bg-muted/30">
                  {t('skills.pluginGroup.general')}
                </div>
                {groupedSelectedSkills.ungrouped.map(renderSkillRow)}
              </div>
            )}
          </>
        ) : (
          state.selectedSkills.map(renderSkillRow)
        )}
      </div>

      {/* 安装目录 — 通用目录始终存储原始文件，Agent 目录根据 mode 符号链接或复制 */}
      <div className="space-y-1.5">
        <span className="text-sm text-muted-foreground">{t('addSkill.confirm.directories')}</span>
        {/* 通用目录 — 原始文件所在 */}
        <div className="border rounded-md">
          <div className="flex items-center justify-between gap-2 px-3 py-2">
            <code className="font-mono text-[13px] text-foreground truncate">
              {universalDir}
            </code>
            <Badge variant="secondary" className="text-xs px-1.5 py-0 shrink-0">
              {t('addSkill.confirm.universal')}
            </Badge>
          </div>
        </div>
        {/* 关系标注 + Agent 目录 */}
        {selectedNonUniversalAgents.length > 0 && (
          <>
            <div className="flex items-center gap-1.5 px-1 text-xs text-muted-foreground">
              <span>↓</span>
              <span>
                {state.mode === 'symlink'
                  ? t('addSkill.confirm.symlink')
                  : t('addSkill.confirm.copy')}
              </span>
            </div>
            <div className="border rounded-md divide-y divide-border/50">
              {selectedNonUniversalAgents.map((agent) => (
                <div key={agent.id} className="flex items-center justify-between gap-2 px-3 py-2">
                  <code className="font-mono text-[13px] text-foreground truncate">
                    {scope === 'global' ? agent.globalSkillsDir : agent.skillsDir}
                  </code>
                  <Badge variant="secondary" className="text-xs px-1.5 py-0 shrink-0">
                    {agent.name}
                  </Badge>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
