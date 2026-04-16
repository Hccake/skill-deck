// src/components/skills/add-skill/OptionsStep.tsx
import { useEffect, useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { listAgents, getLastSelectedAgents } from '@/hooks/useTauriApi';
import { AgentSelector } from './AgentSelector';
import { getEffectiveInstallMode, shouldShowInstallModeSelection, type WizardState } from './types';

// CLI 默认选中的 Non-Universal agents
const DEFAULT_NON_UNIVERSAL_AGENTS = ['claude-code', 'cursor'];

interface OptionsStepProps {
  state: WizardState;
  updateState: (updates: Partial<WizardState>) => void;
}

export function OptionsStep({ state, updateState }: OptionsStepProps) {
  const { t } = useTranslation();

  // 使用 ref 保存 updateState，避免将其作为 useEffect 依赖（advanced-event-handler-refs）
  const updateStateRef = useRef(updateState);
  useEffect(() => { updateStateRef.current = updateState; });

  // 使用 ref 保存 preSelectedAgents，避免将其作为 useEffect 依赖
  const preSelectedAgentsRef = useRef(state.preSelectedAgents);
  useEffect(() => { preSelectedAgentsRef.current = state.preSelectedAgents; });

  // 初始化 agents 数据 — async-parallel 规则
  useEffect(() => {
    async function initAgents() {
      const [allAgents, lastSelected] = await Promise.all([
        listAgents(),
        getLastSelectedAgents(),
      ]);

      // js-set-map-lookups 规则
      const detectedNonUniversalIds = new Set<string>(
        allAgents
          .filter((a) => a.detected && !a.isUniversal)
          .map((a) => a.id)
      );

      let selectedAgents: string[];

      // 优先使用从 CLI 命令解析出的 preSelectedAgents
      if (preSelectedAgentsRef.current.length > 0) {
        const matched = preSelectedAgentsRef.current.filter((id) =>
          allAgents.some((a) => a.id === id && !a.isUniversal)
        );
        selectedAgents = matched.length > 0 ? matched : [];
      } else if (lastSelected.length > 0) {
        selectedAgents = lastSelected.filter(
          (id) => allAgents.some((a) => a.id === id && !a.isUniversal)
        );
      } else {
        selectedAgents = DEFAULT_NON_UNIVERSAL_AGENTS.filter((id) =>
          detectedNonUniversalIds.has(id)
        );
      }

      updateStateRef.current({
        allAgents,
        selectedAgents,
      });
    }

    initAgents();
  }, []);

  const handleSelectionChange = useCallback(
    (agents: string[]) => {
      updateState({ selectedAgents: agents });
    },
    [updateState]
  );

  const shouldShowModeSelection = shouldShowInstallModeSelection(state);
  const effectiveMode = getEffectiveInstallMode(state);

  return (
    <div className="space-y-6 py-4">
      {/* Agents */}
      <AgentSelector
        selectedAgents={state.selectedAgents}
        allAgents={state.allAgents}
        onSelectionChange={handleSelectionChange}
      />

      {/* Mode */}
      {shouldShowModeSelection ? (
        <div className="space-y-3">
          <Label className="text-sm font-medium">{t('addSkill.mode.title')}</Label>
          <RadioGroup
            value={effectiveMode}
            onValueChange={(value) =>
              updateState({ mode: value as 'symlink' | 'copy' })
            }
            className="space-y-2"
          >
            <div className="flex items-start gap-3">
              <RadioGroupItem value="symlink" id="mode-symlink" className="mt-1" />
              <div>
                <Label htmlFor="mode-symlink" className="font-medium">
                  {t('addSkill.mode.symlink')}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t('addSkill.mode.symlinkHint')}
                </p>
              </div>
            </div>
            <div className="flex items-start gap-3">
              <RadioGroupItem value="copy" id="mode-copy" className="mt-1" />
              <div>
                <Label htmlFor="mode-copy" className="font-medium">
                  {t('addSkill.mode.copy')}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t('addSkill.mode.copyHint')}
                </p>
              </div>
            </div>
          </RadioGroup>
        </div>
      ) : (
        <div className="rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
          {t('addSkill.mode.singleDirectoryHint')}
        </div>
      )}
    </div>
  );
}
