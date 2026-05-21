// src/components/skills/add-skill/ConfirmStep.tsx
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, CornerDownRight, FolderGit2, Box, Folder, Package } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import { Skeleton } from '@/components/ui/skeleton';
import { getAgentTarget, getSharedSkillDirectory, isAdditionalAgent, isAutomaticAgent } from '@/lib/agentTargets';
import { checkOverwrites, checkSkillAudit } from '@/hooks/useTauriApi';
import type { SkillAuditData } from '@/hooks/useTauriApi';
import { RiskBadge } from '../RiskBadge';
import { getEffectiveInstallMode, type WizardState } from './types';

function formatPath(path: string) {
  return path
    .replace(/^([A-Z]:\\Users\\[^\\]+|^\/Users\/[^/]+|^\/home\/[^/]+)/i, '~')
    .replace(/[\\/]+$/, '');
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
  const confirmRequestIdRef = useRef(0);

  // 审计数据（组件级 state，不影响 wizard 流程）
  const [auditData, setAuditData] = useState<Partial<Record<string, SkillAuditData>>>({});

  // 并行检测覆盖 + 获取审计数据
  useEffect(() => {
    const requestId = ++confirmRequestIdRef.current;
    let cancelled = false;

    if (state.selectedSkills.length === 0) {
      updateStateRef.current({ overwrites: {}, confirmReady: true });
      return;
    }

    updateStateRef.current({ confirmReady: false });

    const overwriteAgentIds = Array.from(new Set([
      ...state.selectedAgents,
      ...state.allAgents
        .filter((agent) => isAutomaticAgent(agent, scope))
        .map((agent) => agent.id),
    ]));

    const overwritePromise: Promise<Record<string, string[]>> = overwriteAgentIds.length > 0
      ? checkOverwrites(
          state.selectedSkills,
          overwriteAgentIds,
          scope,
          scope === 'project' ? projectPath : undefined
        )
      : Promise.resolve({});

    const auditPromise = state.source
      ? checkSkillAudit(state.source, state.selectedSkills).catch(() => null)
      : Promise.resolve(null);

    Promise.all([overwritePromise, auditPromise]).then(([overwriteResult, auditResult]) => {
      if (cancelled || requestId !== confirmRequestIdRef.current) return;

      const overwrites: Record<string, string[]> = {};
      for (const [key, value] of Object.entries(overwriteResult)) {
        if (value) overwrites[key] = value;
      }

      setAuditData((current) =>
        auditResult ?? (Object.keys(current).length > 0 ? {} : current)
      );
      updateStateRef.current({ overwrites, confirmReady: true });
    }).catch((error) => {
      if (cancelled || requestId !== confirmRequestIdRef.current) return;

      console.error('Failed to check overwrites/audit:', error);
      setAuditData((current) => Object.keys(current).length > 0 ? {} : current);
      updateStateRef.current({ overwrites: {}, confirmReady: true });
    });

    return () => {
      cancelled = true;
    };
  }, [state.selectedSkills, state.selectedAgents, state.allAgents, state.source, scope, projectPath]);

  // 覆盖统计
  const availableSkillMap = useMemo(
    () => new Map(state.availableSkills.map((s) => [s.name, s])),
    [state.availableSkills]
  );

  const overwriteCount = useMemo(
    () => state.selectedSkills.filter((name) => (state.overwrites[name] ?? []).length > 0).length,
    [state.selectedSkills, state.overwrites]
  );

  // 已选的手动安装目标信息（用于目录列表）
  const selectedAdditionalAgents = useMemo(() => {
    const selectedSet = new Set(state.selectedAgents);
    return state.allAgents.filter((agent) =>
      selectedSet.has(agent.id) && isAdditionalAgent(agent, scope)
    );
  }, [state.selectedAgents, state.allAgents, scope]);
  const effectiveMode = getEffectiveInstallMode(state);

  const sharedDir = getSharedSkillDirectory(scope);

  const renderSkillRow = (skillName: string) => {
    const skill = availableSkillMap.get(skillName);
    const overwriteAgents = state.overwrites[skillName] ?? [];
    const hasOverwrite = overwriteAgents.length > 0;
    const trustTypeKey = skill?.wellKnownEntryType === 'legacy'
      ? 'addSkill.confirm.trust.legacy'
      : skill?.wellKnownEntryType === 'skill-md'
        ? 'addSkill.confirm.trust.skillMd'
        : skill?.wellKnownEntryType === 'archive'
          ? 'addSkill.confirm.trust.archive'
          : null;

    return (
      <div key={skillName}>
        <div className="flex items-center justify-between gap-3 px-3 py-2.5">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <Package className="w-4 h-4 text-muted-foreground/70 shrink-0" />
            <span className="min-w-0 max-w-[280px] truncate font-mono text-[13px] text-foreground" title={skillName}>
              {skillName}
            </span>
            <div className="flex items-center gap-1.5 flex-wrap">
              {hasOverwrite && (
                <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-amber-500/30 bg-amber-500/5 text-amber-700 dark:text-amber-400">
                  {t('addSkill.confirm.overwriteGroup')}
                </Badge>
              )}
              {trustTypeKey && (
                <Badge variant="outline" className="text-[10px] px-1.5 py-0 text-muted-foreground bg-muted/20">
                  {t(trustTypeKey)}
                </Badge>
              )}
              {skill?.artifactUrlHost && (
                <Badge variant="secondary" className="text-[10px] px-1.5 py-0 inline-flex items-center gap-1" title={skill.artifactUrlHost}>
                  <Box className="w-2.5 h-2.5 opacity-60" />
                  <span className="truncate max-w-[80px]">{skill.artifactUrlHost}</span>
                </Badge>
              )}
              {skill?.digestVerified && (
                <Badge variant="outline" className="text-[10px] px-1.5 py-0 text-emerald-600 dark:text-emerald-400 border-emerald-500/30 bg-emerald-500/5">
                  {t('addSkill.confirm.trust.digestVerified')}
                </Badge>
              )}
            </div>
          </div>
          <div className="flex-shrink-0 flex items-center gap-2">
            {auditData[skillName] && (
              <RiskBadge risk={auditData[skillName].risk} />
            )}
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className="space-y-4">
      {state.riskPolicy?.kind === 'require-confirmation' && (
        <div className="rounded-md border border-warning/40 bg-warning/10 px-3 py-3 space-y-2">
          <div className="flex items-start gap-2">
            <AlertTriangle className="h-4 w-4 shrink-0 mt-0.5 text-warning" />
            <div className="space-y-1">
              <p className="text-sm font-medium text-foreground">
                {t('addSkill.risk.openclawTitle')}
              </p>
              <p className="text-sm text-muted-foreground">
                {t('addSkill.risk.openclawBody')}
              </p>
            </div>
          </div>
          <label className="flex items-start gap-2 text-sm text-foreground cursor-pointer">
            <Checkbox
              checked={state.riskAcknowledged}
              onCheckedChange={(checked) => updateState({ riskAcknowledged: checked === true })}
              className="mt-0.5"
            />
            <span>{t('addSkill.risk.openclawAcknowledge')}</span>
          </label>
        </div>
      )}

      <div className="space-y-2">
        <div className="space-y-0.5" data-install-contents-section>
          <span className="text-sm font-semibold text-foreground" data-skill-list-heading>
            {t('addSkill.confirm.itemsTitle')}
          </span>
          {state.confirmReady && (
            <p className="text-xs leading-5 text-muted-foreground">
              {overwriteCount > 0
                ? t('addSkill.confirm.summary', {
                    count: state.selectedSkills.length,
                    overwriteCount,
                  })
                : t('addSkill.confirm.summaryNoOverwrite', {
                    count: state.selectedSkills.length,
                  })}
            </p>
          )}
        </div>
        {!state.confirmReady ? (
          <div className="border rounded-md divide-y divide-border/50 bg-card">
            {state.selectedSkills.map((_, idx) => (
              <div key={idx} className="flex items-center justify-between gap-2 px-3 py-3">
                <Skeleton className="h-4 w-32" />
                <Skeleton className="h-5 w-14 rounded-full" />
              </div>
            ))}
          </div>
        ) : (
          <div className="overflow-hidden rounded-md border border-border/60 bg-card">
            <div className="divide-y divide-border/50">
              {state.selectedSkills.map(renderSkillRow)}
            </div>
          </div>
        )}
      </div>

      {/* 物理安装路径拓扑树 */}
      <div className="space-y-2 pt-3">
        <div className="space-y-0.5">
          <span className="text-sm font-semibold text-foreground">{t('addSkill.confirm.directories')}</span>
          <p className="text-xs leading-5 text-muted-foreground">
            {t('addSkill.confirm.directoryHint')}
          </p>
        </div>

        <div className="mt-2 overflow-x-auto rounded-md border border-border/50 bg-muted/20 p-3 font-mono text-[13px]">
          {/* 共享源目录 */}
          <div className="flex items-center gap-2 text-foreground relative z-10">
            <Folder className="h-4 w-4 text-blue-500 dark:text-blue-400 shrink-0" />
            <span className="font-semibold">{formatPath(sharedDir)}</span>
            <Badge variant="secondary" className="text-[10px] px-1.5 py-0 h-4 ml-1 opacity-80 leading-none flex items-center">
              {t('addSkill.confirm.shared')}
            </Badge>
          </div>

          {/* 目标 Agent 依赖节点 */}
          {selectedAdditionalAgents.length > 0 && (
            <div className="mt-1 flex flex-col relative ml-2.5">
              {selectedAdditionalAgents.map((agent, idx) => {
                const isLast = idx === selectedAdditionalAgents.length - 1;
                return (
                  <div key={agent.id} className="relative flex items-center py-1">
                    <div className="absolute left-0 top-0 bottom-0 w-[14px]">
                       <div className="absolute left-0 top-0 w-full h-[50%] border-l-2 border-b-2 border-border/60 rounded-bl-sm" />
                       {!isLast && <div className="absolute left-0 top-[50%] bottom-0 border-l-2 border-border/60" />}
                    </div>
                    <div className="flex items-center gap-2 ml-[22px] mt-px text-muted-foreground w-full">
                      {effectiveMode === 'symlink' ? (
                        <CornerDownRight className="h-3.5 w-3.5 text-orange-500/80 shrink-0" />
                      ) : (
                        <FolderGit2 className="h-3.5 w-3.5 text-emerald-500/80 shrink-0" />
                      )}
                      <span className="truncate">{formatPath(getAgentTarget(agent, scope).path)}</span>
                      <Badge variant="outline" className="text-[10px] px-1.5 py-0 h-[18px] bg-background border-border/80 text-muted-foreground whitespace-nowrap leading-none flex items-center">
                        {agent.name}
                      </Badge>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* 模式图例提示 */}
        {selectedAdditionalAgents.length > 0 && (
          <div className="text-[11px] text-muted-foreground/70 flex items-center gap-1.5 mt-1 px-1">
            {effectiveMode === 'symlink'
              ? <CornerDownRight className="h-3 w-3" />
              : <FolderGit2 className="h-3 w-3" />
            }
            {effectiveMode === 'symlink'
              ? t('addSkill.confirm.symlinkHint')
              : t('addSkill.confirm.copyHint')}
          </div>
        )}
      </div>
    </div>
  );
}
