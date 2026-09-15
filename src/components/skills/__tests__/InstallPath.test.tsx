/* @vitest-environment jsdom */
import '@/test-utils';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { InstallPath } from '../InstallPath';
import { TooltipProvider } from '@/components/ui/tooltip';

vi.mock('react-i18next', () => ({ useTranslation: () => ({
  t: (key: string, values?: { path?: string }) => values?.path ? `${key}:${values.path}` : key,
}) }));

const environment = { kind: 'native' } as const;
const root = { environment, nativePath: '/work/app' };

describe('InstallPath', () => {
  it('opens the full address with a keyboard and copies that address', async () => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue(undefined);
    const path = { environment, nativePath: '/work/app/.claude/skills/toolkit' };
    render(<TooltipProvider><InstallPath path={path} base={{ logicalRoot: root, physicalRoot: root, pathStyle: 'posix' }} scope="project" /></TooltipProvider>);
    const trigger = screen.getByRole('button', { name: 'skills.installPath.viewFullPath:.claude/skills/toolkit' });
    expect(screen.queryByText(path.nativePath)).toBeNull();

    await user.tab();
    expect(document.activeElement).toBe(trigger);
    expect((await screen.findByRole('tooltip')).textContent).toContain(path.nativePath);
    await user.click(screen.getByRole('button', { name: 'skills.installPath.copyPath' }));
    await waitFor(() => expect(copy).toHaveBeenCalledWith(path.nativePath));
    expect(screen.getByRole('status').textContent).toContain('skills.installPath.copied');
  });

  it('shows the full outside address without duplicating scope badges', () => {
    const path = { environment, nativePath: '/elsewhere/toolkit' };
    render(<TooltipProvider><InstallPath path={path} base={{ logicalRoot: root, physicalRoot: root, pathStyle: 'posix' }} scope="project" /></TooltipProvider>);
    expect(screen.queryByText('skills.installPath.outsideProject')).toBeNull();
    expect(screen.getByRole('button', { name: 'skills.installPath.viewFullPath:/elsewhere/toolkit' })).not.toBeNull();
  });
});
