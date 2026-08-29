/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { RedirectHostConfirmation } from '../RedirectHostConfirmation';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: { host?: string }) => (
      values?.host ? `${key}:${values.host}` : key
    ),
  }),
}));

describe('RedirectHostConfirmation', () => {
  it('names the final host and requires an explicit acknowledgement', async () => {
    const user = userEvent.setup();
    const onAcknowledgedChange = vi.fn();
    render(
      <RedirectHostConfirmation
        host="cdn.example.net"
        acknowledged={false}
        onAcknowledgedChange={onAcknowledgedChange}
      />,
    );

    expect(screen.getByText('addSkill.confirm.redirectBody:cdn.example.net')).toBeTruthy();
    await user.click(screen.getByRole('checkbox', {
      name: 'addSkill.confirm.redirectAcknowledge',
    }));
    expect(onAcknowledgedChange).toHaveBeenCalledWith(true);
  });
});
