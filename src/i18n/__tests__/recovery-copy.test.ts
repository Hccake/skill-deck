import { describe, expect, it } from 'vitest';
import zhCN from '../locales/zh-CN.json';

describe('Recovery 中文文案', () => {
  it('说明本次操作未完成，而不是归因于自动恢复', () => {
    expect(zhCN.recovery.title).toBe('本次操作未完成');
    expect(zhCN.recovery.center.title).toBe('需要处理的问题');
    expect(zhCN.recovery.center.description).toBe('查看未完成操作留下的文件和处理记录。');
  });

  it('使用具体、可执行的人工处理动作', () => {
    expect(zhCN.recovery.open).toBe('打开相关文件');
    expect(zhCN.recovery.refresh).toBe('重新检查');
    expect(zhCN.recovery.cleanup).toBe('清理记录');
    expect(zhCN.recovery.confirmCleanup).toBe('清理记录');
  });

  it('不再使用恢复数据或自动恢复失败等误导性表述', () => {
    const copy = JSON.stringify(zhCN.recovery);
    expect(copy).not.toContain('恢复数据');
    expect(copy).not.toContain('自动恢复');
  });
});
