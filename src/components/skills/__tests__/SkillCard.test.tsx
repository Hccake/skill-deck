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

  it('shows cannot-check hint without duplicating it in the title row when no update is available', () => {
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
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('skills.updateStatusLabel.needsSourceInfo')).toBeNull();
    expect(screen.getByText('skills.updateHint.missing-skill-path')).toBeTruthy();
    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('keeps the single skill update action as the primary action when an update is available', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: true,
            canRunUpdate: true,
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.getByTitle('skills.actions.update')).toBeTruthy();
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeTruthy();
  });

  it('keeps agent badges visible when showing a maintenance hint', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              updateReason: 'missing-remote-hash',
              agents: ['claude-code', 'codex'],
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          displayScope="global"
          agentDisplayNames={new Map([
            ['claude-code', 'Claude Code'],
            ['codex', 'Codex'],
          ])}
        />
      </TooltipProvider>
    );

    expect(screen.queryByText('skills.updateStatusLabel.versionUnknown')).toBeNull();
    const hint = screen.getByText('skills.updateHint.missing-remote-hash');
    expect(hint).toBeTruthy();
    expect(hint.className).toContain('bg-muted/25');
    expect(screen.getByText('Claude Code')).toBeTruthy();
    expect(screen.getByText('Codex')).toBeTruthy();
  });

  it('shows repair source action for missing skill path metadata', () => {
    const onRepairSource = vi.fn();

    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: false,
              canCheckForUpdates: false,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'missing-skill-path',
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          displayScope="global"
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.actions.repairSource'));

    expect(onRepairSource).toHaveBeenCalledWith(expect.objectContaining({ name: 'toolkit' }));
  });

  it('uses direct reinstall for missing version metadata', () => {
    const onUpdate = vi.fn();
    const onRepairSource = vi.fn();

    render(
      <TooltipProvider>
        <SkillCard
          skill={{
            ...makeSkill({
              hasUpdate: false,
              canRunUpdate: true,
              canCheckForUpdates: false,
              source: 'owner/repo',
              sourceUrl: 'https://github.com/owner/repo',
              updateReason: 'missing-remote-hash',
            }),
            updateStatus: 'cannot-check',
          } as InstalledSkill & { updateStatus?: 'cannot-check' }}
          displayScope="global"
          onUpdate={onUpdate}
          onRepairSource={onRepairSource}
        />
      </TooltipProvider>
    );

    fireEvent.click(screen.getByTitle('skills.actions.reinstall'));

    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByText('skills.reinstallConfirm.title')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'skills.reinstallConfirm.confirm' }));

    expect(onUpdate).toHaveBeenCalledWith('toolkit');
    expect(onRepairSource).not.toHaveBeenCalled();
  });

  it('hides ordinary update action when update cannot run even if stale update state is present', () => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: true,
            canRunUpdate: false,
            updateReason: 'missing-skill-path',
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it('hides update action for manual-only sources when no update is available', () => {
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
        />
      </TooltipProvider>
    );

    expect(screen.queryByTitle('skills.actions.update')).toBeNull();
  });

  it.each([
    ['rate-limited', 'skills.updateHint.rate-limited'],
    ['auth', 'skills.updateHint.auth'],
    ['network-error', 'skills.updateHint.network-error'],
    ['http-404', 'skills.updateHint.http-error'],
  ])('shows GitHub update reason %s', (reason, expectedKey) => {
    render(
      <TooltipProvider>
        <SkillCard
          skill={makeSkill({
            hasUpdate: false,
            updateReason: reason,
          })}
          displayScope="global"
        />
      </TooltipProvider>
    );

    expect(screen.getByText(expectedKey)).toBeTruthy();
  });
});
