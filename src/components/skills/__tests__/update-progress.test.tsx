/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SkillScope } from '@/bindings';

import {
  phaseToI18nKey,
  phaseToPercent,
  useSkillUpdateProgressListener,
} from '../update-progress';

const eventMocks = vi.hoisted(() => ({
  callback: null as null | ((event: {
    payload: {
      skillName: string;
      scope: SkillScope;
      projectPath?: string | null;
      phase: 'cloning' | 'installing' | 'writing_lock';
    };
  }) => void),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((_: string, callback: typeof eventMocks.callback) => {
    eventMocks.callback = callback;
    return Promise.resolve(() => {
      eventMocks.callback = null;
    });
  }),
}));

function Probe({
    skillName,
    scope,
    projectPath,
    enabled = false,
    onPhase,
  }: {
  skillName: string;
  scope: SkillScope;
  projectPath?: string;
  enabled?: boolean;
  onPhase: (phase: 'cloning' | 'installing' | 'writing_lock') => void;
}) {
  useSkillUpdateProgressListener({
    skillName,
    scope,
    projectPath,
    enabled,
    onPhase,
  });

  return null;
}

describe('update-progress helpers', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.callback = null;
  });

  it('maps progress phases to labels and widths', () => {
    expect(phaseToI18nKey(null)).toBe('skills.updatePhaseCloning');
    expect(phaseToI18nKey('cloning')).toBe('skills.updatePhaseCloning');
    expect(phaseToI18nKey('installing')).toBe('skills.updatePhaseInstalling');
    expect(phaseToI18nKey('writing_lock')).toBe('skills.updatePhaseWritingLock');

    expect(phaseToPercent(null)).toBe('10%');
    expect(phaseToPercent('cloning')).toBe('35%');
    expect(phaseToPercent('installing')).toBe('70%');
    expect(phaseToPercent('writing_lock')).toBe('90%');
  });

  it('forwards matching update-progress events to the listener callback', () => {
    const onPhase = vi.fn();

    render(
      <Probe
        skillName="toolkit"
        scope="global"
        enabled
        onPhase={onPhase}
      />
    );

    act(() => {
      eventMocks.callback?.({
        payload: {
          skillName: 'toolkit',
          scope: 'global',
          phase: 'writing_lock',
        },
      });
    });

    expect(onPhase).toHaveBeenCalledWith('writing_lock');
  });

  it('ignores update-progress events for a different skill identity', () => {
    const onPhase = vi.fn();

    render(
      <Probe
        skillName="toolkit"
        scope="global"
        projectPath="D:\\Code\\project-a"
        enabled
        onPhase={onPhase}
      />
    );

    act(() => {
      eventMocks.callback?.({
        payload: {
          skillName: 'toolkit',
          scope: 'project',
          projectPath: 'D:\\Code\\project-b',
          phase: 'installing',
        },
      });
    });

    expect(onPhase).not.toHaveBeenCalled();
  });
});
