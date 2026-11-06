/* @vitest-environment jsdom */
import '@/test-utils';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import { describe, expect, it, vi } from 'vitest';
import zhCN from '@/i18n/locales/zh-CN.json';
import en from '@/i18n/locales/en.json';
import { RedirectHostConfirmation } from '../RedirectHostConfirmation';

describe('RedirectHostConfirmation', () => {
  it.each([
    ['zh-CN', 'install', '允许继续安装'],
    ['en', 'add', 'Allow adding to the library'],
  ] as const)('describes both hosts and limits consent to this operation in %s', async (language, action, label) => {
    const i18n = createInstance();
    await i18n.init({ lng: language, resources: { 'zh-CN': { translation: zhCN }, en: { translation: en } } });
    const changed = vi.fn();
    const user = userEvent.setup();
    render(<I18nextProvider i18n={i18n}><RedirectHostConfirmation
      sourceUrl="https://docs.example.com/skill.zip?token=temporary-secret"
      host="cdn.example.net" action={action} acknowledged={false} onAcknowledgedChange={changed}
    /></I18nextProvider>);
    const checkbox = screen.getByRole('checkbox', { name: label });
    const description = document.getElementById(checkbox.getAttribute('aria-describedby') ?? '');
    expect(description?.textContent).toContain('docs.example.com');
    expect(description?.textContent).toContain('cdn.example.net');
    expect(document.body.textContent).not.toContain('temporary-secret');
    await user.click(checkbox);
    expect(changed).toHaveBeenCalledWith(true);
  });
});
