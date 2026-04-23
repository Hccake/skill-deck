import type { TFunction } from 'i18next';
import type { AppError, AvailableSkill } from '@/bindings';
import type { InstallError } from '@/components/skills/add-skill/types';

interface ErrorContext {
  selectedSkills?: string[];
  availableSkills?: AvailableSkill[];
}

type ErrorWithMessage = Extract<
  AppError,
  | { kind: 'gitCloneFailed'; data: { message: string } }
  | { kind: 'gitAuthFailed'; data: { message: string } }
  | { kind: 'io'; data: { message: string } }
  | { kind: 'installFailed'; data: { message: string } }
  | { kind: 'yaml'; data: { message: string } }
  | { kind: 'json'; data: { message: string } }
  | { kind: 'invalidSkillMd'; data: { message: string } }
  | { kind: 'path'; data: { message: string } }
>;

type ErrorWithValue = Extract<AppError, { kind: 'invalidSource'; data: { value: string } }>;
type ErrorWithAgent = Extract<AppError, { kind: 'invalidAgent'; data: { agent: string } }>;
type ErrorWithPath = Extract<AppError, { kind: 'pathNotFound'; data: { path: string } }>;
type ErrorWithRepo = Extract<AppError, { kind: 'gitRepoNotFound'; data: { repo: string } }>;
type ErrorWithRef = Extract<AppError, { kind: 'gitRefNotFound'; data: { refName: string } }>;
type ErrorWithRiskCode = Extract<
  AppError,
  { kind: 'installRiskConfirmationRequired'; data: { code: string } }
>;
type CustomError = Extract<AppError, { kind: 'custom'; data: { message: string } }>;
type ErrorWithTimeout = {
  kind: 'gitTimeout';
  data?: {
    timeoutSecs?: number;
  };
};

/**
 * 将结构化 AppError 转换为用户友好的 InstallError 视图模型
 */
export function parseInstallError(
  error: AppError,
  t: TFunction,
  context: ErrorContext = {}
): InstallError {
  const { selectedSkills = [], availableSkills = [] } = context;
  const errorKind = (error as AppError & { kind: string }).kind;

  switch (errorKind) {
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
        details: (error as ErrorWithMessage).data.message,
        suggestions: [
          t('addSkill.error.suggestion.checkNetwork'),
          t('addSkill.error.suggestion.checkRepo'),
          t('addSkill.error.suggestion.checkPrivateRepo'),
        ],
      };

    case 'gitAuthFailed':
      return {
        message: t('addSkill.error.authFailed'),
        details: (error as ErrorWithMessage).data.message,
        suggestions: [
          t('addSkill.error.suggestion.checkCredentials'),
          t('addSkill.error.suggestion.checkAccess'),
        ],
      };

    case 'gitRepoNotFound':
      return {
        message: t('addSkill.error.repoNotFound'),
        details: (error as ErrorWithRepo).data.repo,
        suggestions: [
          t('addSkill.error.suggestion.checkRepo'),
          t('addSkill.error.suggestion.checkAccess'),
        ],
      };

    case 'gitRefNotFound':
      return {
        message: t('addSkill.error.refNotFound'),
        details: (error as ErrorWithRef).data.refName,
        suggestions: [
          t('addSkill.error.suggestion.checkRef'),
          t('addSkill.error.suggestion.useDefaultBranch'),
        ],
      };

    case 'gitTimeout':
      return {
        message: t('addSkill.error.cloneTimeout'),
        details: t('addSkill.error.cloneTimeoutDetails', {
          timeout: (error as ErrorWithTimeout).data?.timeoutSecs ?? 120,
        }),
        suggestions: [
          t('addSkill.error.suggestion.adjustCloneTimeout'),
          t('addSkill.error.suggestion.checkNetwork'),
        ],
      };

    case 'io':
      return {
        message: t('addSkill.error.ioFailed'),
        details: (error as ErrorWithMessage).data.message,
        suggestions: [
          t('addSkill.error.suggestion.runAsAdmin'),
          t('addSkill.error.suggestion.checkPermission'),
        ],
      };

    case 'invalidAgent':
      return {
        message: t('addSkill.error.invalidAgent', { agent: (error as ErrorWithAgent).data.agent }),
        suggestions: [
          t('addSkill.error.suggestion.checkAgentName'),
          t('addSkill.error.suggestion.reselectAgents'),
        ],
      };

    case 'invalidSource':
      return {
        message: t('addSkill.error.invalidSource', { value: (error as ErrorWithValue).data.value }),
        suggestions: [
          t('addSkill.error.suggestion.checkRepo'),
        ],
      };

    case 'pathNotFound':
      return {
        message: t('addSkill.error.pathNotFound', { path: (error as ErrorWithPath).data.path }),
        suggestions: [
          t('addSkill.error.suggestion.checkPermission'),
        ],
      };

    case 'installFailed':
      return {
        message: t('addSkill.error.installFailed'),
        details: (error as ErrorWithMessage).data.message,
        suggestions: [
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };

    case 'installRiskConfirmationRequired':
      return {
        message: t('addSkill.error.riskConfirmationRequired'),
        details: (error as ErrorWithRiskCode).data.code,
        suggestions: [
          t('addSkill.error.suggestion.reviewRiskAndConfirm'),
        ],
      };

    case 'custom':
      return {
        message: (error as CustomError).data.message,
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
        details: (error as ErrorWithMessage).data.message,
        suggestions: [
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };

    default: {
      const _exhaustive: never = errorKind as never;
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
