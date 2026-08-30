import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe('Skill Library application copy', () => {
  it('names the Scope command and its location-specific dialogs', () => {
    expect(zhCN.libraries.manage).toBe('管理 Skill 库');
    expect(zhCN.libraries.manageGlobal).toBe('管理全局 Skill 库');
    expect(zhCN.libraries.manageProject).toBe('管理「{{name}}」的 Skill 库');
    expect(zhCN.libraries.manageProjectFallback).toBe('管理项目 Skill 库');
    expect(zhCN.libraries.manageDescription)
      .toBe('选择并排列此位置使用的 Skill 库，同时设置需要关联的 Agent。');
    expect(zhCN.libraries.appliedSection).toBe('应用的 Skill 库');
    expect(zhCN.libraries.availableSection).toBe('未应用的 Skill 库');
    expect(zhCN.libraries.save).toBe('保存');
    expect(zhCN.libraries.saving).toBe('正在保存…');
    expect(zhCN.libraries.saveError).toBe('无法保存 Skill 库设置，请重试。');
    expect(zhCN.libraries.targetConflictAgent)
      .toBe('{{agents}} 的专用 Skill 目录中存在无法安全处理的 {{skill}}，请取消关联后再保存。');
    expect(zhCN.libraries.targetConflictScope)
      .toBe('通用 Skill 目录中的 {{skill}} 不是可管理的 Skill，请处理该目录项后再保存。');
    expect(zhCN.libraries.cancelConflictingAgents).toBe('取消关联 {{agents}}');
    expect(zhCN.libraries.continue).toBe('继续完成更改');
    expect(zhCN.libraries.copyOnlyUnsupported)
      .toBe('{{names}} 需要复制或转换 Skill，无法关联 Skill 库。');

    expect(en.libraries.manage).toBe('Manage Skill Libraries');
    expect(en.libraries.manageGlobal).toBe('Manage Global Skill Libraries');
    expect(en.libraries.manageProject).toBe('Manage Skill Libraries for “{{name}}”');
    expect(en.libraries.manageProjectFallback).toBe('Manage Project Skill Libraries');
    expect(en.libraries.appliedSection).toBe('Applied Skill Libraries');
    expect(en.libraries.availableSection).toBe('Unapplied Skill Libraries');
    expect(en.libraries.save).toBe('Save');
    expect(en.libraries.saving).toBe('Saving…');
    expect(en.libraries.saveError).toBe('Skill Library settings could not be saved. Try again.');
    expect(en.libraries.targetConflictAgent)
      .toBe('{{agents}} has an unsupported {{skill}} entry in its private Skill directory. Remove the association before saving.');
    expect(en.libraries.targetConflictScope)
      .toBe('{{skill}} in the shared Skill directory cannot be managed. Resolve the entry before saving.');
    expect(en.libraries.cancelConflictingAgents).toBe('Remove {{agents}} association');
  });

  it('keeps Skill placement conflicts actionable in both languages', () => {
    expect(zhCN.mutation.result.targetKinds.file).toBe('文件');
    expect(zhCN.mutation.result.targetKinds.other).toBe('其他目录项');
    expect(zhCN.mutation.result.errors.skillPlacementTargetConflict)
      .toBe('{{skillName}} 的目标 Skill 目录 {{targetPath}} 中存在无法管理的{{targetKind}}，请处理后重试。');
    expect(en.mutation.result.targetKinds.file).toBe('file');
    expect(en.mutation.result.targetKinds.other).toBe('directory entry');
    expect(en.mutation.result.errors.skillPlacementTargetConflict)
      .toBe('The target Skill directory {{targetPath}} for {{skillName}} contains a {{targetKind}} that cannot be managed. Resolve it and try again.');
  });
});
