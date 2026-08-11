import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe('proxy settings copy', () => {
  it('uses a precise proxy server label and reserves format guidance for validation', () => {
    expect(zhCN.settings.proxy.address).toBe('代理服务器地址');
    expect(zhCN.settings.proxy.errors.invalidProxyUrl)
      .toBe('请填写 HTTP 或 HTTPS 代理地址，并包含端口。');
    expect(zhCN.settings.proxy).not.toHaveProperty('addressHint');

    expect(en.settings.proxy.address).toBe('Proxy server URL');
    expect(en.settings.proxy.errors.invalidProxyUrl)
      .toBe('Enter an HTTP or HTTPS proxy URL with a port.');
    expect(en.settings.proxy).not.toHaveProperty('addressHint');
  });

  it('uses concise Chinese labels that describe the resulting connection behavior', () => {
    expect(zhCN.settings.proxy.modeTitle).toBe('HTTP 请求');
    expect(zhCN.settings.proxy.connectionMode).toBe('连接方式');
    expect(zhCN.settings.proxy.mode).toEqual({
      custom: '使用代理',
      direct: '直接连接',
    });
    expect(zhCN.settings.proxy.gitTitle).toBe('Git 操作');
    expect(zhCN.settings.proxy.gitBehavior).toEqual({
      useProxy: '使用代理',
      useExistingGitConfig: '保持原有连接方式',
    });
    expect(zhCN.settings.proxy.wslBehavior).toEqual({
      followNativeGit: '与 Windows Git 保持一致',
      useExistingGitConfig: '保持原有连接方式',
      useProxy: '使用代理',
    });
    expect(zhCN.settings.proxy.scope).toEqual({
      githubOnly: '仅 GitHub 仓库',
      allHttpHttps: '所有 HTTP/HTTPS 仓库',
    });
    expect(zhCN.settings.proxy.scopeTitle).toBe('适用仓库');
    expect(zhCN.settings.proxy.test.status.idle).toBe('尚未测试');
    expect(zhCN.settings.proxy.test.status.testing).toBe('正在测试');
    expect(JSON.stringify(zhCN.settings.proxy)).not.toContain('Skill Deck');
  });

  it('keeps the English proxy copy structurally aligned with Chinese', () => {
    expect(en.settings.proxy.mode).toEqual({
      custom: 'Use proxy',
      direct: 'Direct connection',
    });
    expect(en.settings.proxy.gitBehavior).toEqual({
      useProxy: 'Use proxy',
      useExistingGitConfig: 'Keep existing connection method',
    });
    expect(Object.keys(en.settings.proxy.wslBehavior))
      .toEqual(Object.keys(zhCN.settings.proxy.wslBehavior));
    expect(Object.keys(en.settings.proxy.test.status))
      .toEqual(Object.keys(zhCN.settings.proxy.test.status));
  });
});
