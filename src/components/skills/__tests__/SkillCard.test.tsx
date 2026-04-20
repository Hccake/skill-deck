/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SkillCard } from '../SkillCard';
import type { InstalledSkill } from '@/bindings';

const eventMocks = vi.hoisted(() => ({
  callback: null as null | ((event: { payload: { skillName: string; scope?: string; projectPath?: string | null; phase: string } }) => void),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en' },
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((_: string, callback: typeof eventMocks.callback) => {
    eventMocks.callback = callback;
    return Promise.resolve(() => {
      eventMocks.callback = null;
    });
  }),
}));

const makeSkill = (overrides: Partial<InstalledSkill> = {}): InstalledSkill => ({
  name: 'toolkit',
  description: 'Toolkit',
  path: '/skills/toolkit',
  canonicalPath: '/canonical/toolkit',
  scope: 'global',
  agents: [],
  hasUpdate: true,
  ...overrides,
});

describe('SkillCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.callback = null;
  });

  it('ignores update-progress events from a different skill identity', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({ scope: 'global' })}
          displayScope="global"
          updateStatus="updating"
        />
      </TooltipProvider>
    );

    act(() => {
      eventMocks.callback?.({
        payload: {
          skillName: 'toolkit',
          scope: 'project',
          projectPath: 'D:\\Code\\other-project',
          phase: 'writing_lock',
        },
      });
    });

    expect(screen.queryByText('skills.updatePhaseWritingLock')).toBeNull();
    expect(screen.getByText('skills.updatePhaseCloning')).toBeTruthy();
  });

  it('shows cannot-check status and still allows update when canRunUpdate is true', () => {
    const onUpdate = vi.fn();

    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          displayScope="global"
          onUpdate={onUpdate}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updateStatus.cannotCheck')).toBeTruthy();
    fireEvent.click(screen.getByTitle('skills.actions.update'));
    expect(onUpdate).toHaveBeenCalledWith('toolkit');
  });

  it('shows update action for manual-only sources before any update check runs', () => {
    const onUpdate = vi.fn();

    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: false,
            canRunUpdate: true,
            canCheckForUpdates: false,
            updateReason: 'unsupported-source-type',
          })}
          displayScope="global"
          onUpdate={onUpdate}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.actions.update'));
    expect(onUpdate).toHaveBeenCalledWith('toolkit');
  });
});
