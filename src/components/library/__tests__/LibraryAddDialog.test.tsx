/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LibraryAddDialog } from '../LibraryAddDialog';
import type { FetchResult } from '@/bindings';

const api = vi.hoisted(() => ({
  discoverSkillSource: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

vi.mock('@/hooks/useTauriApi', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/hooks/useTauriApi')>(),
  discoverSkillSource: api.discoverSkillSource,
}));

vi.mock('@/components/skills/skill-search/SkillSearch', () => ({
  SkillSearch: ({ actionLabel }: { actionLabel?: string }) => <div>{actionLabel}</div>,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
  }),
}));

const environment = { kind: 'native' } as const;
const target = {
  environment,
  environmentName: 'Windows',
  libraryId: 'lib-1',
  libraryName: 'Backend',
} as const;

const discovery = {
  discoverySession: {
    sessionId: 'session-1',
    environment,
    sourceFingerprint: 'source-1',
    expiresAtEpochMs: 10_000,
  },
  sourceType: 'git',
  sourceUrl: 'https://example.com/repo',
  redirectedDownloadHost: null,
  gitRef: null,
  skillFilter: null,
  skills: [
    {
      name: 'api-design',
      installDirName: 'api-design',
      description: 'Design APIs',
      relativePath: 'skills/api-design',
    },
    {
      name: 'ui-review',
      installDirName: 'ui-review',
      description: 'Review desktop UI',
      relativePath: `skills/${'long-segment-'.repeat(20)}ui-review`,
    },
  ],
} satisfies FetchResult;

describe('LibraryAddDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.discoverSkillSource.mockResolvedValue(discovery);
  });

  it('shows the target Library once and keeps the Environment as target metadata', () => {
    render(
      <LibraryAddDialog
        open
        target={target}
        existingSkillNames={new Set()}
        execute={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getAllByText(/Backend/)).toHaveLength(1);
    expect(screen.getByText('Windows')).toBeTruthy();
  });

  it('names an online search result action as adding a Skill', async () => {
    const user = userEvent.setup();
    render(
      <LibraryAddDialog
        open
        target={target}
        existingSkillNames={new Set()}
        execute={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    await user.click(screen.getByRole('tab', { name: 'addSkill.source.tabs.search' }));

    expect(screen.getByText('libraries.addFlow.source.add')).toBeTruthy();
    expect(screen.queryByText('libraries.addFlow.source.useResult')).toBeNull();
  });

  it('keeps IME composition from submitting and discovers against the captured Environment', async () => {
    const execute = vi.fn();
    render(
      <LibraryAddDialog
        open
        target={target}
        existingSkillNames={new Set(['api-design'])}
        execute={execute}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole('dialog', { name: /libraries\.addFlow\.title/ })).toBeTruthy();
    const input = screen.getByRole('textbox', { name: 'libraries.addFlow.source.label' });
    fireEvent.change(input, { target: { value: 'skills add https://example.com/repo --skill ui-review' } });
    fireEvent.compositionStart(input);
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(api.discoverSkillSource).not.toHaveBeenCalled();

    fireEvent.compositionEnd(input);
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(api.discoverSkillSource).toHaveBeenCalledWith(
      environment,
      'https://example.com/repo',
      expect.any(String),
      { wildcardRequested: false, explicitSkillNames: ['ui-review'] },
    ));
  });

  it('keeps existing members disabled and excludes them from select all', async () => {
    const user = userEvent.setup();
    render(
      <LibraryAddDialog
        open
        target={target}
        existingSkillNames={new Set(['api-design'])}
        execute={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    const input = screen.getByRole('textbox', { name: 'libraries.addFlow.source.label' });
    await user.type(input, 'https://example.com/repo');
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.source.read' }));

    const existing = await screen.findByRole('checkbox', { name: /api-design/ });
    expect(existing).toHaveProperty('disabled', true);
    expect(screen.getByText('libraries.addFlow.selection.alreadyInLibrary')).toBeTruthy();

    const footer = screen.getByRole('dialog').querySelector<HTMLElement>('[data-slot="dialog-footer"]');
    if (!footer) throw new Error('dialog footer is missing');
    expect(within(footer).getAllByRole('button').map((button) => button.textContent)).toEqual([
      'common.cancel',
      'addSkill.actions.back',
      'libraries.addFlow.selection.review',
    ]);

    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.selection.selectAll' }));
    expect(existing.getAttribute('data-state')).toBe('unchecked');
    expect(screen.getByRole('checkbox', { name: /ui-review/ }).getAttribute('data-state')).toBe('checked');
  });

  it('prepares and executes the selected Skills through the captured Library target', async () => {
    const user = userEvent.setup();
    const execute = vi.fn()
      .mockResolvedValueOnce({
        status: 'succeeded',
        snapshot: {
          pendingAdd: {
            request: { environment, libraryId: 'lib-1', discoverySession: discovery.discoverySession, skills: [] },
            preview: {
              token: {
                generation: 'preview-1',
                contextRevision: 'context-1',
                skillRevisions: [],
                redirectedDownloadHost: null,
              },
              skills: [{ skillName: 'ui-review', targetPath: '/libraries/lib-1/skills/ui-review' }],
              redirectedDownloadHost: null,
            },
          },
          retryAdd: null,
          lastAddResults: [],
        },
      })
      .mockResolvedValueOnce({
        status: 'succeeded',
        snapshot: {
          pendingAdd: null,
          retryAdd: null,
          lastAddResults: [{ skillName: 'ui-review', status: 'succeeded', error: null }],
        },
      });
    render(
      <LibraryAddDialog
        open
        target={target}
        existingSkillNames={new Set(['api-design'])}
        execute={execute}
        onClose={vi.fn()}
      />,
    );

    await user.type(
      screen.getByRole('textbox', { name: 'libraries.addFlow.source.label' }),
      'https://example.com/repo',
    );
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.source.read' }));
    await user.click(await screen.findByRole('checkbox', { name: /ui-review/ }));
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.selection.review' }));

    await waitFor(() => expect(execute).toHaveBeenNthCalledWith(1, {
      kind: 'addSkills',
      libraryId: 'lib-1',
      discovery,
      skillPaths: [discovery.skills[1].relativePath],
    }));
    expect(await screen.findByText('libraries.addFlow.review.summary:{"count":1}')).toBeTruthy();
    expect(screen.queryByText('libraries.addFlow.review.title')).toBeNull();

    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.review.confirm' }));
    await waitFor(() => expect(execute).toHaveBeenNthCalledWith(2, {
      kind: 'confirmAddSkills',
      acknowledgeRedirect: false,
    }));
    expect(await screen.findByText('libraries.addFlow.result.succeeded:{"count":1}')).toBeTruthy();
    expect(screen.getByRole('dialog').querySelector('[aria-current="step"]')).toBeNull();
  });

  it('keeps earlier successes when retrying only the failed Skills', async () => {
    const user = userEvent.setup();
    const preview = {
      token: {
        generation: 'preview-1',
        contextRevision: 'context-1',
        skillRevisions: [],
        redirectedDownloadHost: null,
      },
      skills: discovery.skills.map((skill) => ({
        skillName: skill.name,
        targetPath: `/libraries/lib-1/skills/${skill.name}`,
      })),
      redirectedDownloadHost: null,
    };
    const execute = vi.fn()
      .mockResolvedValueOnce({
        status: 'succeeded',
        snapshot: { pendingAdd: { request: {}, preview }, retryAdd: null, lastAddResults: [] },
      })
      .mockResolvedValueOnce({
        status: 'succeeded',
        snapshot: {
          pendingAdd: {
            request: {},
            preview: {
              ...preview,
              skills: [preview.skills[1]],
            },
          },
          retryAdd: null,
          lastAddResults: [
            { skillName: 'api-design', status: 'succeeded', error: null },
            { skillName: 'ui-review', status: 'failed', error: { kind: 'staleTarget' } },
          ],
        },
      })
      .mockResolvedValueOnce({
        status: 'succeeded',
        snapshot: {
          pendingAdd: null,
          retryAdd: null,
          lastAddResults: [{ skillName: 'ui-review', status: 'succeeded', error: null }],
        },
      });
    render(
      <LibraryAddDialog
        open
        target={target}
        existingSkillNames={new Set()}
        execute={execute}
        onClose={vi.fn()}
      />,
    );

    await user.type(
      screen.getByRole('textbox', { name: 'libraries.addFlow.source.label' }),
      'https://example.com/repo',
    );
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.source.read' }));
    await user.click(await screen.findByRole('button', { name: 'libraries.addFlow.selection.selectAll' }));
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.selection.review' }));
    await user.click(await screen.findByRole('button', { name: 'libraries.addFlow.review.confirm' }));

    expect(await screen.findByText('libraries.addFlow.result.partial:{"succeeded":1,"failed":1}')).toBeTruthy();
    const resultFooter = screen.getByRole('dialog').querySelector<HTMLElement>('[data-slot="dialog-footer"]');
    if (!resultFooter) throw new Error('dialog footer is missing');
    expect(within(resultFooter).getByRole('button', { name: 'common.close' })).toBeTruthy();
    expect(within(resultFooter).queryByRole('button', { name: 'common.cancel' })).toBeNull();
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.result.retry' }));

    expect(await screen.findByText('libraries.addFlow.result.succeeded:{"count":2}')).toBeTruthy();
    expect(screen.getAllByText('libraries.addResult.succeeded')).toHaveLength(2);
  });

  it('requires explicit confirmation before writing a cross-host download', async () => {
    const user = userEvent.setup();
    const execute = vi.fn().mockResolvedValueOnce({
      status: 'succeeded',
      snapshot: {
        pendingAdd: {
          request: {},
          preview: {
            token: {
              generation: 'preview-redirect',
              contextRevision: 'context-1',
              skillRevisions: [],
              redirectedDownloadHost: 'cdn.example.net',
            },
            skills: [{ skillName: 'ui-review', targetPath: '/libraries/lib-1/skills/ui-review' }],
            redirectedDownloadHost: 'cdn.example.net',
          },
        },
        retryAdd: null,
        lastAddResults: [],
      },
    }).mockResolvedValueOnce({
      status: 'succeeded',
      snapshot: {
        pendingAdd: null,
        retryAdd: null,
        lastAddResults: [{ skillName: 'ui-review', status: 'succeeded', error: null }],
      },
    });
    render(
      <LibraryAddDialog
        open
        target={target}
        existingSkillNames={new Set(['api-design'])}
        execute={execute}
        onClose={vi.fn()}
      />,
    );

    await user.type(
      screen.getByRole('textbox', { name: 'libraries.addFlow.source.label' }),
      'https://example.com/repo',
    );
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.source.read' }));
    await user.click(await screen.findByRole('checkbox', { name: /ui-review/ }));
    await user.click(screen.getByRole('button', { name: 'libraries.addFlow.selection.review' }));

    const confirm = await screen.findByRole('button', { name: 'libraries.addFlow.review.confirm' });
    expect(confirm).toHaveProperty('disabled', true);
    await user.click(screen.getByRole('checkbox', { name: 'addSkill.confirm.redirectAcknowledge' }));
    expect(confirm).toHaveProperty('disabled', false);
    await user.click(confirm);

    await waitFor(() => expect(execute).toHaveBeenLastCalledWith({
      kind: 'confirmAddSkills',
      acknowledgeRedirect: true,
    }));
  });
});
