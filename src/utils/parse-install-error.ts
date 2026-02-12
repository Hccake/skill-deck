import type { TFunction } from 'i18next';
import type { AppError, AvailableSkill } from '@/bindings';
import type { InstallError } from '@/components/skills/add-skill/types';

interface ErrorContext {
  selectedSkills?: string[];
  availableSkills?: AvailableSkill[];
}

/**
 * 将结构化 AppError 转换为用户友好的 InstallError 视图模型
 */
export function parseInstallError(
  error: AppError,
  t: TFunction,
  context: ErrorContext = {}
): InstallError {
  const { selectedSkills = [], availableSkills = [] } = context;

  switch (error.kind) {
    case 'noSkillsFound': {
      const availableNames = availableSkills.map(s => s.name);
      return {
        message: t('addSkill.error.noSkillsFound'),
        details: selectedSkills.length > 0 && availableNames.length > 0
          ? t('addSkill.error.noSkillsFoundDetails', {
              selected: selectedSkills.join(', '),
              available: availableNames.join(', '),
            })
          : undefined,
        suggestions: [
          t('addSkill.error.suggestion.checkSkillName'),
          t('addSkill.error.suggestion.reselect'),
        ],
      };
    }

    case 'gitNetworkError':
    case 'gitCloneFailed':
      return {
        message: t('addSkill.error.networkFailed'),
        details: error.data.message,
        suggestions: [
          t('addSkill.error.suggestion.checkNetwork'),
          t('addSkill.error.suggestion.checkRepo'),
          t('addSkill.error.suggestion.checkPrivateRepo'),
        ],
      };

    case 'gitAuthFailed':
      return {
        message: t('addSkill.error.authFailed'),
        details: error.data.message,
        suggestions: [
          t('addSkill.error.suggestion.checkCredentials'),
          t('addSkill.error.suggestion.checkAccess'),
        ],
      };

    case 'gitRepoNotFound':
      return {
        message: t('addSkill.error.repoNotFound'),
        details: error.data.repo,
        suggestions: [
          t('addSkill.error.suggestion.checkRepo'),
          t('addSkill.error.suggestion.checkAccess'),
        ],
      };

    case 'gitRefNotFound':
      return {
        message: t('addSkill.error.refNotFound'),
        details: error.data.refName,
        suggestions: [
          t('addSkill.error.suggestion.checkRef'),
          t('addSkill.error.suggestion.useDefaultBranch'),
        ],
      };

    case 'gitTimeout':
      return {
        message: t('addSkill.error.cloneTimeout'),
        suggestions: [
          t('addSkill.error.suggestion.checkNetwork'),
          t('addSkill.error.suggestion.retryLater'),
        ],
      };

    case 'io':
      return {
        message: t('addSkill.error.ioFailed'),
        details: error.data.message,
        suggestions: [
          t('addSkill.error.suggestion.runAsAdmin'),
          t('addSkill.error.suggestion.checkPermission'),
        ],
      };

    case 'invalidAgent':
      return {
        message: t('addSkill.error.invalidAgent', { agent: error.data.agent }),
        suggestions: [
          t('addSkill.error.suggestion.checkAgentName'),
          t('addSkill.error.suggestion.reselectAgents'),
        ],
      };

    case 'invalidSource':
      return {
        message: t('addSkill.error.invalidSource', { value: error.data.value }),
        suggestions: [
          t('addSkill.error.suggestion.checkRepo'),
        ],
      };

    case 'pathNotFound':
      return {
        message: t('addSkill.error.pathNotFound', { path: error.data.path }),
        suggestions: [
          t('addSkill.error.suggestion.checkPermission'),
        ],
      };

    case 'installFailed':
      return {
        message: t('addSkill.error.installFailed'),
        details: error.data.message,
        suggestions: [
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };

    case 'custom':
      return {
        message: error.data.message,
        suggestions: [
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };

    case 'yaml':
    case 'json':
    case 'invalidSkillMd':
    case 'path':
      return {
        message: t('addSkill.error.parseFailed'),
        details: error.data.message,
        suggestions: [
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };

    default: {
      const _exhaustive: never = error;
      void _exhaustive;
      return {
        message: t('addSkill.error.unknown'),
        suggestions: [
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };
    }
  }
}
