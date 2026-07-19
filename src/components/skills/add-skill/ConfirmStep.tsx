// src/components/skills/add-skill/ConfirmStep.tsx
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  AlertTriangle,
  CornerDownRight,
  FolderGit2,
  Box,
  Folder,
  Package,
  Copy,
  Bot,
  type LucideIcon,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import { Skeleton } from '@/components/ui/skeleton';
import {
  canCreatePrivateCopy,
  getAgentTarget,
  getSharedSkillDirectory,
  isDefaultAvailableAgent,
  isPrivateRequiredAgent,
} from '@/lib/agentTargets';
import { agentDisplayName, agentId } from '@/lib/agents';
import {
  acquireSelectedPayloads,
  checkSkillAudit,
  previewInstall,
} from '@/hooks/useTauriApi';
import type { SkillAuditData } from '@/hooks/useTauriApi';
import type { InstallTargetInfo } from '@/bindings';
import type { AdapterTargetSelection } from '@/lib/install-workflow';
import { RiskBadge } from '../RiskBadge';
import { getEffectiveInstallMode, type WizardState } from './types';
import { buildAgentWriteIntents } from '@/lib/install-workflow';

function formatPath(path: string) {
  return path
    .replace(/^([A-Z]:\\Users\\[^\\]+|^\/Users\/[^/]+|^\/home\/[^/]+)/i, '~')
    .replace(/[\\/]+$/, '');
}

function targetKey(target: InstallTargetInfo | AdapterTargetSelection) {
  return target.targetId;
}

function targetLabel(target: AdapterTargetSelection) {
  return target.targetId;
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

    if (!state.discoverySession) {
      updateStateRef.current({ confirmReady: false });
      return;
    }

    const skillPaths = state.selectedSkills.flatMap((name) => {
      const skill = state.availableSkills.find((candidate) => candidate.name === name);
      return skill ? [skill.relativePath] : [];
    });
    const requestPromise = acquireSelectedPayloads({
      discoverySession: state.discoverySession,
      skillPaths,
    }).then((payloads) => {
      const request = {
        context: state.context,
        source: state.source,
        discoverySession: state.discoverySession!,
        payloads,
        skills: state.selectedSkills,
        agentIntents: buildAgentWriteIntents({
          agents: state.allAgents,
          scope,
          selectedAgents: state.selectedAgents,
          privateCopyAgents: state.privateCopyAgents,
          adapterTargets: state.selectedAgentTargets ?? [],
        }),
        requestedMode: getEffectiveInstallMode({
          allAgents: state.allAgents,
          selectedAgents: state.selectedAgents,
          mode: state.mode,
          scope: state.scope,
        }),
        acknowledgeRisk: state.riskAcknowledged,
      };
      return previewInstall(request).then((preview) => ({ payloads, request, preview }));
    });

    const auditPromise = state.source
      ? checkSkillAudit(state.source, state.selectedSkills).catch(() => null)
      : Promise.resolve(null);

    Promise.all([requestPromise, auditPromise]).then(([install, auditResult]) => {
      if (cancelled || requestId !== confirmRequestIdRef.current) return;

      const overwrites: Record<string, string[]> = {};
      for (const skill of install.preview.skills) {
        if (skill.overwriteTargets.length > 0) {
          overwrites[skill.skillName] = skill.overwriteTargets;
        }
      }

      setAuditData((current) =>
        auditResult ?? (Object.keys(current).length > 0 ? {} : current)
      );
      updateStateRef.current({
        acquiredPayloads: install.payloads,
        installRequest: install.request,
        installPreview: install.preview,
        overwrites,
        confirmReady: true,
      });
    }).catch((error) => {
      if (cancelled || requestId !== confirmRequestIdRef.current) return;

      console.error('Failed to check overwrites/audit:', error);
      setAuditData((current) => Object.keys(current).length > 0 ? {} : current);
      updateStateRef.current({ overwrites: {}, confirmReady: true });
    });

    return () => {
      cancelled = true;
    };
  }, [state.selectedSkills, state.selectedAgents, state.selectedAgentTargets, state.privateCopyAgents, state.allAgents, state.availableSkills, state.discoverySession, state.source, state.context, state.mode, state.scope, state.riskAcknowledged, scope, projectPath]);

  // 覆盖统计
  const availableSkillMap = useMemo(
    () => new Map(state.availableSkills.map((s) => [s.name, s])),
    [state.availableSkills]
  );

  const overwriteCount = useMemo(
    () => state.selectedSkills.filter((name) => (state.overwrites[name] ?? []).length > 0).length,
    [state.selectedSkills, state.overwrites]
  );

  const defaultAvailableAgents = useMemo(() => {
    return state.allAgents.filter((agent) => isDefaultAvailableAgent(agent, scope));
  }, [state.allAgents, scope]);

  const selectedPrivateRequiredAgents = useMemo(() => {
    const selectedSet = new Set(state.selectedAgents);
    return state.allAgents.filter((agent) =>
      selectedSet.has(agentId(agent)) && isPrivateRequiredAgent(agent, scope)
    );
  }, [state.selectedAgents, state.allAgents, scope]);

  const selectedPrivateCopyAgents = useMemo(() => {
    const selectedSet = new Set(state.privateCopyAgents);
    return state.allAgents.filter((agent) =>
      selectedSet.has(agentId(agent)) && canCreatePrivateCopy(agent, scope)
    );
  }, [state.privateCopyAgents, state.allAgents, scope]);
  const selectedConcreteTargets = useMemo(() => {
    const availableByKey = new Map(
      (state.availableAgentTargets ?? []).map((target) => [targetKey(target), target])
    );

    return (state.selectedAgentTargets ?? []).map((target) => {
      const info = availableByKey.get(targetKey(target));
      if (info) return info;
      return {
        targetId: targetKey(target),
        agent: target.agentId,
        displayName: targetLabel(target),
        subagent: null,
        path: '',
      } satisfies InstallTargetInfo;
    });
  }, [state.availableAgentTargets, state.selectedAgentTargets]);
  const effectiveMode = getEffectiveInstallMode(state);

  const sharedDir = getSharedSkillDirectory(scope);

  const renderSkillRow = (skillName: string) => {
    const skill = availableSkillMap.get(skillName);
    const overwriteAgents = state.overwrites[skillName] ?? [];
    const hasOverwrite = overwriteAgents.length > 0;
    const installDirName = skill?.installDirName;
    const hasInstallDirNameChange = Boolean(
      installDirName && installDirName !== skillName
    );
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
        {hasInstallDirNameChange && (
          <div className="mx-3 mb-2 -mt-0.5 rounded-md border border-amber-500/25 bg-amber-500/5 px-3 py-2">
            <div className="flex items-start gap-2">
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
              <div className="min-w-0 space-y-0.5">
                <p className="text-xs font-medium text-foreground">
                  {t('addSkill.confirm.installDirNameChanged')}
                </p>
                <p className="text-xs leading-5 text-muted-foreground">
                  {t('addSkill.confirm.installDirNameChangedHint', {
                    installDirName,
                  })}
                </p>
              </div>
            </div>
          </div>
        )}
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

      {/* 安装计划 */}
      <div className="space-y-2 pt-3">
        <div className="space-y-0.5">
          <span className="text-sm font-semibold text-foreground">{t('addSkill.confirm.installPlan')}</span>
          <p className="text-xs leading-5 text-muted-foreground">
            {t('addSkill.confirm.installPlanHint')}
          </p>
        </div>

        <div className="space-y-2">
          <InstallPlanSection
            icon={Folder}
            title={t('addSkill.confirm.defaultLocation')}
            hint={t('addSkill.confirm.defaultLocationHint')}
            path={formatPath(sharedDir)}
              agents={defaultAvailableAgents.map(agentDisplayName)}
          />

          {selectedPrivateRequiredAgents.length > 0 && (
            <InstallPlanSection
              icon={effectiveMode === 'symlink' ? CornerDownRight : FolderGit2}
              title={t('addSkill.confirm.privateSetup')}
              hint={effectiveMode === 'symlink'
                ? t('addSkill.confirm.symlinkHint')
                : t('addSkill.confirm.copyHint')}
              agents={selectedPrivateRequiredAgents.map(agentDisplayName)}
              paths={selectedPrivateRequiredAgents.map((agent) => formatPath(
                getAgentTarget(agent, scope).privatePath ?? '',
              ))}
            />
          )}

          {selectedPrivateCopyAgents.length > 0 && (
            <InstallPlanSection
              icon={Copy}
              title={t('addSkill.confirm.privateCopies')}
              hint={t('addSkill.confirm.privateCopiesHint')}
              agents={selectedPrivateCopyAgents.map(agentDisplayName)}
              paths={selectedPrivateCopyAgents.map((agent) =>
                formatPath(getAgentTarget(agent, scope).privatePath ?? '')
              )}
            />
          )}

          {selectedConcreteTargets.length > 0 && (
            <InstallPlanSection
              icon={Bot}
              title={t('addSkill.confirm.concreteTargets')}
              hint={t('addSkill.confirm.concreteTargetsHint')}
              agents={selectedConcreteTargets.map((target) => target.displayName)}
              paths={selectedConcreteTargets
                .map((target) => target.path)
                .filter((path) => path.length > 0)
                .map(formatPath)}
            />
          )}
        </div>

        {/* 模式图例提示 */}
        {selectedPrivateRequiredAgents.length > 0 && (
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

function InstallPlanSection({
  icon: Icon,
  title,
  hint,
  path,
  paths,
  agents,
}: {
  icon: LucideIcon;
  title: string;
  hint: string;
  path?: string;
  paths?: string[];
  agents: string[];
}) {
  return (
    <div className="rounded-md border border-border/50 bg-muted/15 px-3 py-2.5">
      <div className="flex items-start gap-2.5">
        <Icon className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1 space-y-1.5">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[13px] font-semibold text-foreground">{title}</span>
            {path && (
              <code className="rounded bg-background/70 px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground">
                {path}
              </code>
            )}
          </div>
          <p className="text-xs leading-5 text-muted-foreground">{hint}</p>
          {paths && paths.length > 0 && (
            <div className="flex flex-col gap-1">
              {paths.map((item) => (
                <code key={item} className="truncate font-mono text-[11px] text-muted-foreground/80">
                  {item}
                </code>
              ))}
            </div>
          )}
          {agents.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {agents.map((agent) => (
                <Badge key={agent} variant="outline" className="h-[18px] px-1.5 py-0 text-[10px]">
                  {agent}
                </Badge>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
