// src/components/skills/add-skill/SourceStep.tsx
import { useCallback, useEffect, useState, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { Info } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { fetchAvailable } from '@/hooks/useTauriApi';
import { parseSkillsCommand } from '@/utils/parse-skills-command';
import { formatAppError } from '@/utils/format-app-error';
import { toAppError } from '@/utils/to-app-error';
import { SkillSearch } from '../skill-search/SkillSearch';
import type { SearchSkill } from '../skill-search/SkillSearch';
import { useSkillsDataStore } from '@/stores/skills-data';
import { contextKey, globalContext } from '@/lib/context';
import type { WizardState } from './types';

const EMPTY_SKILLS: never[] = [];

/** 克隆进度事件 */
interface CloneProgress {
  phase: 'connecting' | 'cloning' | 'done' | 'error';
  elapsed_secs: number;
  timeout_secs: number;
  message: string | null;
}

interface CloneProgressEvent extends CloneProgress {
  operation_id: string;
}

interface SourceStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
  onNext: () => void;
  autoFetch?: boolean;
}

export function SourceStep({ state, updateState, onNext, autoFetch }: SourceStepProps) {
  const { t } = useTranslation();
  const [cloneProgress, setCloneProgress] = useState<CloneProgress | null>(null);
  const currentOperationIdRef = useRef<string | null>(null);

  // 已安装 skill key 集合（用于 SkillSearch 组件）
  const globalKey = contextKey(globalContext(state.context.environment));
  const projectKey = state.context.scope.scope === 'project'
    ? contextKey(state.context)
    : null;
  const globalSkills = useSkillsDataStore((s) => s.snapshots[globalKey]?.skills ?? EMPTY_SKILLS);
  const projectSkills = useSkillsDataStore((s) => (
    projectKey ? s.snapshots[projectKey]?.skills ?? EMPTY_SKILLS : EMPTY_SKILLS
  ));
  const installedSkillKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const s of globalSkills) keys.add(`${s.source ?? ''}::${s.name}`);
    for (const s of projectSkills) keys.add(`${s.source ?? ''}::${s.name}`);
    return keys;
  }, [globalSkills, projectSkills]);

  // 监听克隆进度事件
  useEffect(() => {
    const unlisten = listen<CloneProgressEvent>('clone-progress', (event) => {
      if (event.payload.operation_id !== currentOperationIdRef.current) return;
      setCloneProgress({
        phase: event.payload.phase,
        elapsed_secs: event.payload.elapsed_secs,
        timeout_secs: event.payload.timeout_secs,
        message: event.payload.message,
      });
    });

    return () => {
      currentOperationIdRef.current = null;
      unlisten.then((fn) => fn());
    };
  }, []);

  // 核心 fetch 逻辑，接受 source 参数
  const handleFetchWithSource = useCallback(async (source: string) => {
    if (!source.trim()) {
      currentOperationIdRef.current = null;
      setCloneProgress(null);
      updateState({
        fetchStatus: 'error',
        fetchError: { kind: 'custom', data: { message: t('addSkill.source.error.empty') } },
      });
      return;
    }

    updateState({ fetchStatus: 'loading', fetchError: null });
    setCloneProgress(null);
    const operationId = crypto.randomUUID();
    currentOperationIdRef.current = operationId;

    try {
      // 解析 CLI 命令（如 npx skills add repo --skill x -a y）
      const parsed = parseSkillsCommand(source);
      const actualSource = parsed.isCommand ? parsed.source : source.trim();

      if (!actualSource) {
        currentOperationIdRef.current = null;
        updateState({
          fetchStatus: 'error',
          fetchError: { kind: 'custom', data: { message: t('addSkill.source.error.empty') } },
        });
        return;
      }

      const result = await fetchAvailable(state.context, actualSource, operationId);

      if (currentOperationIdRef.current !== operationId) return;

      if (result.skills.length === 0) {
        updateState({
          fetchStatus: 'error',
          fetchError: { kind: 'noSkillsFound' },
        });
        return;
      }

      // 合并 skillFilter（@skill 语法）和 CLI --skill 参数
      const preselectedFromFilter = result.skillFilter
        ? result.skills
            .filter(s => s.name === result.skillFilter)
            .map(s => s.name)
        : [];
      const preselectedFromCommand = parsed.skills.filter(name =>
        result.skills.some(s => s.name === name)
      );
      const preselected = [...new Set([...preselectedFromFilter, ...preselectedFromCommand])];

      updateState({
        source: actualSource, // 保存解析后的 source（去除命令前缀）
        fetchStatus: 'success',
        availableSkills: result.skills,
        selectedSkills: preselected,
        skillFilter: result.skillFilter,
        gitRef: result.gitRef ?? null,
        discoverySession: result.discoverySession,
        preparation: { status: 'idle' },
        riskPolicy: result.riskPolicy ?? null,
        riskAcknowledged: false,
        preSelectedSkills: parsed.skills,
        preSelectedAgents: parsed.agents,
      });

      // 自动进入下一步
      onNext();
    } catch (error) {
      if (currentOperationIdRef.current !== operationId) return;
      updateState({
        fetchStatus: 'error',
        fetchError: toAppError(error),
        riskPolicy: null,
        riskAcknowledged: false,
        discoverySession: undefined,
        preparation: { status: 'idle' },
      });
    }
  }, [updateState, onNext, state.context, t]);

  const handleFetch = useCallback(() => {
    handleFetchWithSource(state.source);
  }, [handleFetchWithSource, state.source]);

  // autoFetch 仅在 fetchStatus 为 idle（从未 fetch 过）时触发
  // 回退再进入时 fetchStatus 已非 idle，不会重复触发，用户可自由修改 source
  useEffect(() => {
    if (autoFetch && state.fetchStatus === 'idle' && state.source) {
      const frameId = requestAnimationFrame(() => {
        handleFetchWithSource(state.source);
      });

      return () => cancelAnimationFrame(frameId);
    }
  }, [autoFetch, state.fetchStatus, state.source, handleFetchWithSource]);

  // 搜索结果选中处理（用于 SkillSearch 组件）
  const handleSearchSelect = useCallback((skill: SearchSkill) => {
    const newSource = `${skill.source}@${skill.name}`;
    updateState({ source: newSource });
    handleFetchWithSource(newSource);
  }, [updateState, handleFetchWithSource]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && state.fetchStatus !== 'loading') {
      handleFetch();
    }
  }, [handleFetch, state.fetchStatus]);

  const isLoading = state.fetchStatus === 'loading';

  // 获取阶段文字
  const getPhaseText = () => {
    if (!cloneProgress) return t('addSkill.source.status.cloning');
    switch (cloneProgress.phase) {
      case 'connecting':
        return t('addSkill.source.status.connecting');
      case 'cloning':
        return t('addSkill.source.status.cloningWithTime', {
          elapsed: cloneProgress.elapsed_secs,
          timeout: cloneProgress.timeout_secs,
        });
      case 'done':
        return t('addSkill.source.status.cloneDone');
      default:
        return t('addSkill.source.status.cloning');
    }
  };

  return (
    <div className="flex flex-col gap-4 h-full">
      <Tabs defaultValue="manual" className="flex flex-col flex-1 min-h-0">
        <div className="flex justify-center mb-6">
          <TabsList className="w-full max-w-sm grid grid-cols-2 p-1 bg-muted/50 rounded-xl">
            <TabsTrigger value="search" className="rounded-lg" disabled={isLoading}>
              {t('addSkill.source.tabs.search')}
            </TabsTrigger>
            <TabsTrigger value="manual" className="rounded-lg" disabled={isLoading}>
              {t('addSkill.source.tabs.manual')}
            </TabsTrigger>
          </TabsList>
        </div>

        {isLoading ? (
          /* 统一加载视图 — 替换所有 Tab 内容，无论从哪个 tab 触发都可见 */
          <div className="flex flex-col items-center justify-center flex-1 space-y-4 animate-in fade-in zoom-in-95 duration-300">
            <div className="w-10 h-10 border-4 border-primary/20 border-t-primary rounded-full animate-spin" />
            <div className="text-center space-y-2">
              <p className="text-sm font-medium text-foreground">{getPhaseText()}</p>
              <p className="text-xs text-muted-foreground font-mono truncate max-w-[280px] bg-muted/30 px-2 py-1 rounded-md">
                {state.source.replace(/@[^@]+$/, '')}
              </p>
            </div>
          </div>
        ) : (
          <div className="flex-1 animate-in fade-in duration-300 flex flex-col">
            {/* 搜索 Tab */}
            <TabsContent value="search" className="flex-1 min-h-0 m-0">
              <div className="h-full px-2">
                <SkillSearch
                  installedSkillKeys={installedSkillKeys}
                  onInstall={handleSearchSelect}
                />
              </div>
            </TabsContent>

            {/* 手动输入 Tab */}
            <TabsContent value="manual" className="m-0 px-2 mt-2">
              <div className="relative group">
                <Input
                  value={state.source}
                  onChange={(e) => updateState({ source: e.target.value })}
                  onKeyDown={handleKeyDown}
                  placeholder={t('addSkill.source.placeholder')}
                  className="w-full h-14 pl-5 pr-[120px] text-base bg-card/80 backdrop-blur-sm border-muted-foreground/20 shadow-sm group-focus-within:shadow-md focus-visible:ring-primary/20 rounded-2xl transition-all"
                />
                <div className="absolute right-1.5 top-1.5 bottom-1.5">
                  <Button
                    onClick={handleFetch}
                    disabled={!state.source.trim()}
                    className="h-full px-6 shadow-sm rounded-xl font-medium"
                  >
                    {t('addSkill.source.actions.fetch')}
                  </Button>
                </div>
              </div>

              <div className="flex items-center gap-2 mt-4 ml-1 text-xs text-muted-foreground/70">
                <Info className="h-4 w-4 shrink-0" />
                <p>{t('addSkill.source.hint')}</p>
              </div>

              {state.gitRef ? (
                <div className="flex items-center gap-2 mt-4 ml-1">
                  <Badge variant="secondary" className="bg-secondary/50 font-medium">
                    {t('addSkill.source.badges.ref', { ref: state.gitRef })}
                  </Badge>
                  {state.skillFilter ? (
                    <Badge variant="outline" className="border-primary/20 font-medium">
                      {t('addSkill.source.badges.skillFilter', { filter: state.skillFilter })}
                    </Badge>
                  ) : null}
                </div>
              ) : null}
            </TabsContent>
          </div>
        )}
      </Tabs>

      {/* Error 在 Tabs 外层 — 无论从哪个 tab 触发的错误都能显示 */}
      {state.fetchStatus === 'error' && state.fetchError && (
        <div className="p-3 bg-destructive/10 text-destructive text-sm rounded-md whitespace-pre-wrap">
          {formatAppError(state.fetchError, t)}
        </div>
      )}
    </div>
  );
}
