/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Button } from '../button';
import { PopConfirm } from '../pop-confirm';

describe('PopConfirm', () => {
  it('requires an explicit confirm before running the action', () => {
    const onConfirm = vi.fn();

    render(
      <PopConfirm
        title="Reinstall skill?"
        description="This will reinstall the skill from its source."
        confirmLabel="Reinstall"
        cancelLabel="Cancel"
        onConfirm={onConfirm}
      >
        <Button>Open</Button>
      </PopConfirm>
    );

    fireEvent.click(screen.getByRole('button', { name: 'Open' }));

    expect(screen.getByText('Reinstall skill?')).toBeTruthy();
    expect(onConfirm).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Reinstall' }));

    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('Reinstall skill?')).toBeNull();
  });

  it('closes without running the action when cancelled', () => {
    const onConfirm = vi.fn();

    render(
      <PopConfirm
        title="Reinstall skill?"
        confirmLabel="Reinstall"
        cancelLabel="Cancel"
        onConfirm={onConfirm}
      >
        <Button>Open</Button>
      </PopConfirm>
    );

    fireEvent.click(screen.getByRole('button', { name: 'Open' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(onConfirm).not.toHaveBeenCalled();
    expect(screen.queryByText('Reinstall skill?')).toBeNull();
  });
});
