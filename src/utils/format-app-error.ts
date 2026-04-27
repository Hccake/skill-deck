import type { TFunction } from 'i18next';
import type { AppError } from '@/bindings';

/**
 * 将 AppError 格式化为用户可读的字符串（用于 SourceStep 等简单错误展示）
 */
export function formatAppError(error: AppError, t: TFunction): string {
  switch (error.kind) {
    case 'custom':
      return error.data.message;
    case 'noSkillsFound':
      return t('addSkill.source.error.noSkills');
    case 'gitTimeout':
      return t('addSkill.source.error.timeout', {
        timeout: error.data.timeoutSecs,
      });
    case 'gitAuthFailed':
      return t('addSkill.source.error.auth');
    case 'gitRepoNotFound':
      return t('addSkill.source.error.notFound');
    case 'gitRefNotFound':
      return t('addSkill.source.error.refNotFound');
    case 'gitNetworkError':
    case 'gitCloneFailed':
    case 'gitHubApiError':
      // GitHubApiError 主要在 update 流程出现 (检查 skill folder hash 时),
      // install 路径基本走不到。此处仅为 exhaustive match 兜底,统一归到 network 文案。
      return t('addSkill.source.error.network');
    case 'invalidSource':
      return t('addSkill.source.error.invalidSource', { value: error.data.value });
    case 'invalidAgent':
      return t('addSkill.error.invalidAgent', { agent: error.data.agent });
    case 'pathNotFound':
      return t('addSkill.error.pathNotFound', { path: error.data.path });
    case 'installFailed':
      return error.data.message;
    case 'installRiskConfirmationRequired':
      return t('addSkill.error.riskConfirmationRequired');
    case 'io':
    case 'yaml':
    case 'json':
    case 'invalidSkillMd':
    case 'path':
      return error.data.message;
    default: {
      const _exhaustive: never = error;
      void _exhaustive;
      return t('addSkill.error.unknown');
    }
  }
}
