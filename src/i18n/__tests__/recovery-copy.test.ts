import { describe, expect, it } from 'vitest';
import zhCN from '../locales/zh-CN.json';

describe('Recovery 中文文案', () => {
  it('说明本次操作未完成，而不是归因于自动恢复', () => {
    expect(zhCN.recovery.title).toBe('本次操作未完成');
    expect(zhCN.recovery.center.title).toBe('需要处理的操作');
    expect(zhCN.recovery.center.description)
      .toBe('这些操作未能确认文件状态。完成处理后可重新发起原操作。');
  });

  it('使用具体、可执行的人工处理动作', () => {
    expect(zhCN.recovery.openDirectory).toBe('打开处理目录');
    expect(zhCN.recovery.openRecordDirectory).toBe('打开诊断目录');
    expect(zhCN.recovery.refresh).toBe('重新检查');
    expect(zhCN.recovery.cleanup).toBe('完成处理');
    expect(zhCN.recovery.confirmCleanup).toBe('删除备份并完成');
  });

  it('准确说明完成处理会删除备份并保留当前 Skill', () => {
    expect(zhCN.recovery.cleanupTitle).toBe('删除备份并完成处理？');
    expect(zhCN.recovery.cleanupDescription).toBe(
      '将删除本次操作保留的备份和处理记录。目标位置的当前文件不会被删除。',
    );
  });

  it('不再使用恢复数据或自动恢复失败等误导性表述', () => {
    const copy = JSON.stringify(zhCN.recovery);
    expect(copy).not.toContain('恢复数据');
    expect(copy).not.toContain('自动恢复');
  });
});
