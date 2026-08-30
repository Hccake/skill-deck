/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { LibraryUsageLine } from '../LibraryUsageLine';
import type { LibraryUsage, RegisteredProject } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

const project = (
  id: string,
  nativePath: string,
  displayName: string | null = null,
): RegisteredProject => ({
  id,
  nativePath,
  displayName,
  order: null,
  suppressCrossStorageWarning: false,
});

const globalUsage = (
  state: LibraryUsage['state'] = 'confirmed',
): LibraryUsage => ({
  context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
  project: null,
  state,
});

const projectUsage = (
  binding: RegisteredProject,
  state: LibraryUsage['state'] = 'confirmed',
): LibraryUsage => ({
  context: {
    environment: { kind: 'native' },
    scope: { scope: 'project', project_id: binding.id },
  },
  project: binding,
  state,
});

const renderLine = (usages: LibraryUsage[]) => render(
  <TooltipProvider><LibraryUsageLine usages={usages} /></TooltipProvider>
);

describe('LibraryUsageLine', () => {
  it('names the only location and keeps a count form for compact workspaces', () => {
    renderLine([globalUsage()]);

    expect(screen.getByText('libraries.appliedTo')).toBeTruthy();
    expect(screen.getByText('libraries.usage.globalLocation')).toBeTruthy();
    expect(screen.getByText('libraries.usage.applied:{"count":1}')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', {
      name: 'libraries.usage.viewDetails:{"summary":"libraries.usage.appliedToLocation:{\\"location\\":\\"libraries.usage.globalLocation\\"}"}',
    }));
    expect(screen.getAllByText('libraries.usage.globalLocation')).toHaveLength(2);
  });

  it('folds several locations into a count with a bounded width', () => {
    renderLine([
      globalUsage(),
      projectUsage(project('project-1', '/work/skill-deck', 'Skill Deck')),
      projectUsage(project('project-2', 'C:\\Code\\my-web-app')),
    ]);

    // 与其他元信息共用一行，位置名不能无限堆叠。
    expect(screen.getByText('libraries.usage.applied:{"count":3}')).toBeTruthy();
    expect(screen.queryByText('Skill Deck')).toBeNull();

    fireEvent.click(screen.getByRole('button', {
      name: 'libraries.usage.viewDetails:{"summary":"libraries.usage.applied:{\\"count\\":3}"}',
    }));
    expect(screen.getByText('Skill Deck')).toBeTruthy();
    expect(screen.getByText('/work/skill-deck')).toBeTruthy();
    expect(screen.getByText('my-web-app')).toBeTruthy();
    expect(screen.getByText('C:\\Code\\my-web-app')).toBeTruthy();
  });

  it('uses the count form as soon as an adjustment is unfinished', () => {
    // 单个已确认位置，但存在未完成调整——不能只显示位置名而吞掉警告。
    renderLine([
      globalUsage(),
      projectUsage(project('project-1', '/work/skill-deck', 'Skill Deck'), 'pendingAdjustment'),
    ]);

    expect(screen.getByText('libraries.usage.applied:{"count":1}')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', {
      name: 'libraries.usage.viewDetails:{"summary":"libraries.usage.appliedWithPending:{\\"count\\":1}"}',
    }));
    expect(screen.getByText('libraries.pendingAdjustment')).toBeTruthy();
  });

  it('reports a pending-only library as an unfinished adjustment', () => {
    renderLine([globalUsage('pendingAdjustment')]);

    expect(screen.getByText('libraries.usage.pendingOnly')).toBeTruthy();
  });

  it('states that an unapplied library is not in effect', () => {
    renderLine([]);

    expect(screen.getByText('libraries.usage.unapplied')).toBeTruthy();
  });
});
