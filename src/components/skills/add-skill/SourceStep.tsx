// src/components/skills/add-skill/SourceStep.tsx
import { useCallback, useEffect, useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import { fetchAvailable } from '@/hooks/useTauriApi';
import { parseSkillsCommand } from '@/utils/parse-skills-command';
import { formatAppError } from '@/utils/format-app-error';
import { toAppError } from '@/utils/to-app-error';
import { SkillSearch } from '../skill-search';
import type { SearchSkill } from '../skill-search';
import { useSkillsStore } from '@/stores/skills';
import type { WizardState } from './types';

/** 克隆进度事件 */
interface CloneProgress {
  phase: 'connecting' | 'cloning' | 'done' | 'error';
  elapsed_secs: number;
  timeout_secs: number;
  message: string | null;
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

  // 已安装 skill key 集合（用于 SkillSearch 组件）
  const globalSkills = useSkillsStore((s) => s.globalSkills);
  const projectSkills = useSkillsStore((s) => s.projectSkills);
  const installedSkillKeys = useMemo(() => {
    const keys = new Set<string>();
    for (const s of globalSkills) keys.add(`${s.source ?? ''}::${s.name}`);
    for (const s of projectSkills) keys.add(`${s.source ?? ''}::${s.name}`);
    return keys;
  }, [globalSkills, projectSkills]);

  // 监听克隆进度事件
  useEffect(() => {
    const unlisten = listen<CloneProgress>('clone-progress', (event) => {
      setCloneProgress(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // 核心 fetch 逻辑，接受 source 参数
  const handleFetchWithSource = useCallback(async (source: string) => {
    if (!source.trim()) {
      updateState({
        fetchStatus: 'error',
        fetchError: { kind: 'custom', data: { message: t('addSkill.source.error.empty') } },
      });
      return;
    }

    updateState({ fetchStatus: 'loading', fetchError: null });
    setCloneProgress(null);

    try {
      // 解析 CLI 命令（如 npx skills add repo --skill x -a y）
      const parsed = parseSkillsCommand(source);
      const actualSource = parsed.isCommand ? parsed.source : source.trim();

      if (!actualSource) {
        updateState({
          fetchStatus: 'error',
          fetchError: { kind: 'custom', data: { message: t('addSkill.source.error.empty') } },
        });
        return;
      }

      const result = await fetchAvailable(actualSource);

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
        preSelectedSkills: parsed.skills,
        preSelectedAgents: parsed.agents,
      });

      // 自动进入下一步
      onNext();
    } catch (error) {
      updateState({
        fetchStatus: 'error',
        fetchError: toAppError(error),
      });
    }
  }, [updateState, onNext, t]);

  const handleFetch = useCallback(() => {
    handleFetchWithSource(state.source);
  }, [handleFetchWithSource, state.source]);

  // autoFetch 仅在 fetchStatus 为 idle（从未 fetch 过）时触发
  // 回退再进入时 fetchStatus 已非 idle，不会重复触发，用户可自由修改 source
  useEffect(() => {
    if (autoFetch && state.fetchStatus === 'idle' && state.source) {
      requestAnimationFrame(() => {
        handleFetch();
      });
    }
  }, [autoFetch, state.fetchStatus, state.source, handleFetch]);

  // 搜索结果选中处理（用于 SkillSearch 组件）
  const handleSearchSelect = useCallback((skill: SearchSkill) => {
    const newSource = `${skill.source}@${skill.name}`;
    updateState({ source: newSource });
    setTimeout(() => {
      handleFetchWithSource(newSource);
    }, 0);
  }, [updateState, handleFetchWithSource]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && state.fetchStatus !== 'loading') {
      handleFetch();
    }
  }, [handleFetch, state.fetchStatus]);

  const isLoading = state.fetchStatus === 'loading';

  // 计算进度百分比
  const progressPercent = cloneProgress
    ? Math.min((cloneProgress.elapsed_secs / cloneProgress.timeout_secs) * 100, 99)
    : 0;

  // 获取阶段文字
  const getPhaseText = () => {
    if (!cloneProgress) return t('addSkill.source.cloning');
    switch (cloneProgress.phase) {
      case 'connecting':
        return t('addSkill.source.connecting');
      case 'cloning':
        return t('addSkill.source.cloningWithTime', {
          elapsed: cloneProgress.elapsed_secs,
          timeout: cloneProgress.timeout_secs,
        });
      case 'done':
        return t('addSkill.source.cloneDone');
      default:
        return t('addSkill.source.cloning');
    }
  };

  return (
    <div className="space-y-4 py-4">
      <Tabs defaultValue="manual">
        <TabsList className="w-full">
          <TabsTrigger value="search" className="flex-1">
            {t('addSkill.source.tabs.search')}
          </TabsTrigger>
          <TabsTrigger value="manual" className="flex-1">
            {t('addSkill.source.tabs.manual')}
          </TabsTrigger>
        </TabsList>

        {/* 搜索 Tab */}
        <TabsContent value="search">
          <div className="h-64">
            <SkillSearch
              installedSkillKeys={installedSkillKeys}
              onInstall={handleSearchSelect}
            />
          </div>
        </TabsContent>

        {/* 手动输入 Tab（原有内容） */}
        <TabsContent value="manual">
          <div className="space-y-2">
            <label className="text-sm font-medium">
              {t('addSkill.source.label')}
            </label>
            <div className="flex gap-2">
              <Input
                value={state.source}
                onChange={(e) => updateState({ source: e.target.value })}
                onKeyDown={handleKeyDown}
                placeholder={t('addSkill.source.placeholder')}
                disabled={isLoading}
                className="flex-1"
              />
              <Button
                onClick={handleFetch}
                disabled={isLoading || !state.source.trim()}
              >
                {isLoading ? t('addSkill.source.fetching') : t('addSkill.source.fetch')}
              </Button>
            </div>
            <p className="text-sm text-muted-foreground">
              {t('addSkill.source.hint')}
            </p>
          </div>

          {/* Error message */}
          {state.fetchStatus === 'error' && state.fetchError && (
            <div className="p-3 bg-destructive/10 text-destructive text-sm rounded-md whitespace-pre-wrap mt-2">
              {formatAppError(state.fetchError, t)}
            </div>
          )}

          {/* Loading indicator with progress */}
          {isLoading && (
            <div className="space-y-2 mt-2">
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <div className="w-4 h-4 border-2 border-primary border-t-transparent rounded-full animate-spin" />
                <span>{getPhaseText()}</span>
              </div>
              {cloneProgress && cloneProgress.phase === 'cloning' && (
                <Progress value={progressPercent} className="h-1" />
              )}
            </div>
          )}
        </TabsContent>
      </Tabs>
    </div>
  );
}
