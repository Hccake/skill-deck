export type InstallScope = 'global' | 'project';

const SHARED_SKILL_DIRECTORIES: Record<InstallScope, string> = {
  global: '~/.agents/skills',
  project: './.agents/skills',
};

export function getSharedSkillDirectory(scope: InstallScope) {
  return SHARED_SKILL_DIRECTORIES[scope];
}
