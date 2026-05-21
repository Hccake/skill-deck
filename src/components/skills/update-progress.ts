import { useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getSkillIdentity, isSameSkillIdentity } from '@/lib/skills/identity';
import type { SkillScope } from '@/bindings';

export type SkillUpdatePhase = 'cloning' | 'installing' | 'writing_lock';

export function phaseToPercent(phase: string | null): string {
  switch (phase) {
    case 'cloning':
      return '35%';
    case 'installing':
      return '70%';
    case 'writing_lock':
      return '90%';
    default:
      return '10%';
  }
}

export function phaseToI18nKey(phase: string | null): string {
  switch (phase) {
    case 'cloning':
      return 'skills.updatePhaseCloning';
    case 'installing':
      return 'skills.updatePhaseInstalling';
    case 'writing_lock':
      return 'skills.updatePhaseWritingLock';
    default:
      return 'skills.updatePhaseCloning';
  }
}

export function useSkillUpdateProgressListener({
  skillName,
  scope,
  projectPath,
  enabled,
  onPhase,
}: {
  skillName: string;
  scope: SkillScope;
  projectPath?: string;
  enabled: boolean;
  onPhase: (phase: SkillUpdatePhase) => void;
}) {
  const onPhaseRef = useRef(onPhase);

  useEffect(() => {
    onPhaseRef.current = onPhase;
  }, [onPhase]);

  useEffect(() => {
    if (!enabled) return;

    const currentIdentity = getSkillIdentity({ name: skillName, scope }, projectPath);
    const unlisten = listen<{ skillName: string; scope: SkillScope; projectPath?: string | null; phase: SkillUpdatePhase }>(
      'update-progress',
      (event) => {
        if (isSameSkillIdentity(currentIdentity, {
          name: event.payload.skillName,
          scope: event.payload.scope,
          projectPath: event.payload.projectPath,
        })) {
          onPhaseRef.current(event.payload.phase);
        }
      }
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [enabled, projectPath, skillName, scope]);
}
