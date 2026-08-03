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
    case 'installRiskConfirmationRequired':
      return t('addSkill.error.riskConfirmationRequired');
    case 'mutationBusy':
      return t('addSkill.error.mutationBusy');
    case 'installWizardActive':
      return t('addSkill.error.installWizardActive');
    case 'installWizardSessionUnavailable':
      return t('addSkill.error.installWizardSessionUnavailable');
    case 'applicationTerminating':
      return t('addSkill.error.applicationTerminating');
    case 'wslIntegrationBusy': {
      const messageKey = {
        mutation: 'settings.general.wslBusyMutation',
        lifecycle: 'settings.general.wslBusyLifecycle',
        installWizard: 'settings.general.wslBusyInstallWizard',
        wslOperation: 'settings.general.wslBusyOperation',
      }[error.data.reason];
      return t(messageKey);
    }
    case 'mutationCancelled':
      return t('addSkill.error.mutationCancelled');
    case 'environmentDiscoveryFailed':
      return t('addSkill.error.environmentDiscoveryFailed');
    case 'wslCommandTimedOut':
      return t('addSkill.error.wslCommandTimedOut');
    case 'wslOutputLimitExceeded':
      return t('addSkill.error.wslOutputLimitExceeded', {
        stream: error.data.stream,
        limit: error.data.limit,
      });
    case 'wslCommandFailed':
      return t('addSkill.error.wslCommandFailed', {
        exitCode: error.data.exitCode ?? t('common.unknown'),
      });
    case 'environmentUnavailable':
      return t('addSkill.error.environmentUnavailable');
    case 'storageMappingUnsupported':
      return t('addSkill.error.storageMappingUnsupported', {
        path: error.data.path,
      });
    case 'projectMigrationFailed':
      return error.data.message;
    case 'configurationReadOnly':
      return t('addSkill.error.configurationReadOnly');
    case 'payloadStorageRequiresCleanup':
      return t('addSkill.error.payloadStorageRequiresCleanup');
    case 'validation':
      return error.data.message;
    case 'environmentChanged':
    case 'contextChanged':
    case 'payloadSessionExpired':
    case 'staleContext':
    case 'staleRegistry':
    case 'staleEnvironment':
    case 'stalePayload':
    case 'staleTarget':
    case 'staleAgentRuntime':
      return t('addSkill.error.staleState');
    case 'storageUnsupported':
    case 'unsafePath':
    case 'unsafeSourceLink':
      return t('addSkill.error.storageUnsupported');
    case 'capabilityUnavailable':
      if (error.kind === 'capabilityUnavailable'
        && (error.data.capability === 'runtimeMaintenancePending'
          || error.data.capability === 'runtimeMaintenanceFailed')) {
        return t('addSkill.error.runtimeMaintenance');
      }
      return t('addSkill.error.storageUnsupported');
    case 'selfCopy':
      return t('addSkill.error.selfCopy');
    case 'externalLockChanged':
      return t('addSkill.error.externalLockChanged');
    case 'executionFailed':
    case 'restoreFailed':
    case 'recoveryRequired':
    case 'configurationCorrupted':
      return error.data.message;
    case 'lockConflict': {
      const { target } = error.data;
      return target.kind === 'skill'
        ? t('addSkill.error.lockConflict', { skill: target.skillName })
        : t('addSkill.error.agentDefaultsConflict');
    }
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
