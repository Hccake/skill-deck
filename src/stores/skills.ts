// src/stores/skills.ts — re-export barrel for backward compatibility
// During migration, consumers can import from here or from specific stores.
export { useSkillsDataStore } from './skills-data';
export { useSkillDetailStore } from './skill-detail';
export { useSkillDialogStore } from './skill-dialog';
export type { DeleteTarget, AddDialogPrefill } from './skills-utils';
