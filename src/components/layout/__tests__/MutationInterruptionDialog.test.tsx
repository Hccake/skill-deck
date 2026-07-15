/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { LifecycleAction } from '@/bindings';
import { MutationInterruptionDialog } from '../MutationInterruptionDialog';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe.each<[LifecycleAction, string, string]>([
  ['closeCurrentWindow', 'closeWindowTitle', 'cancelAndCloseWindow'],
  ['quitApplication', 'quitTitle', 'cancelAndQuit'],
  ['restartApplication', 'restartTitle', 'cancelAndRestart'],
])('MutationInterruptionDialog for %s', (action, titleKey, cancelKey) => {
  it('renders action-specific title and cancellation copy', () => {
    render(
      <MutationInterruptionDialog
        open
        action={action}
        statusText="Installing"
        cancelable
        cancelling={false}
        onContinueWaiting={vi.fn()}
        onCancelAndContinue={vi.fn()}
      />,
    );

    expect(screen.getByText(`mutation.interruption.${titleKey}`)).toBeDefined();
    expect(screen.getByRole('button', {
      name: `mutation.interruption.${cancelKey}`,
    })).toBeDefined();
  });
});
