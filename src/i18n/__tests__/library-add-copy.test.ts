import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';

describe('Library add flow copy', () => {
  it('describes collection membership instead of direct installation in Chinese', () => {
    expect(zhCN.libraries.addFlow.title).toBe('添加到「{{library}}」');
    expect(zhCN.libraries.addFlow.source.label).toBe('Skill 来源');
    expect(zhCN.libraries.addFlow.source.add).toBe('添加');
    expect(zhCN.libraries.addFlow.review.summary).toBe('将添加 {{count}} 个 Skill');
    expect(zhCN.libraries.addFlow.steps.review).toBe('核对并添加');
    expect(zhCN.libraries.addFlow.selection.review).toBe('核对所选 Skill');
    expect(zhCN.libraries.addFlow.review.confirm).toBe('添加 Skill');
    expect(zhCN.libraries.addFlow.selection.alreadyInLibrary).toBe('已在库中');
    expect(zhCN.libraries.addFlow.selection.agentIntentIgnored).toBe(
      '命令中的 Agent 参数不适用于 Skill 库；添加后可在 Skills 页应用此库。',
    );
  });

  it('keeps the English fallback aligned with the same Library semantics', () => {
    expect(en.libraries.addFlow.title).toBe('Add to “{{library}}”');
    expect(en.libraries.addFlow.source.label).toBe('Skill source');
    expect(en.libraries.addFlow.source.add).toBe('Add');
    expect(en.libraries.addFlow.review.summary).toBe('Add {{count}} Skills');
    expect(en.libraries.addFlow.steps.review).toBe('Review & Add');
    expect(en.libraries.addFlow.selection.review).toBe('Review Selected Skills');
    expect(en.libraries.addFlow.review.confirm).toBe('Add Skills');
    expect(en.libraries.addFlow.selection.alreadyInLibrary).toBe('Already in Library');
    expect(en.libraries.addFlow.selection.agentIntentIgnored).toBe(
      'Agent options in this command do not apply to a Skill Library. Apply the Library from the Skills page after adding.',
    );
  });
});
