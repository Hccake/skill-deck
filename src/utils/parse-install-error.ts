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

    case 'gitCloneFailed':
      return {
        message: t('addSkill.error.gitFailed'),
        details: error.data.message,
        suggestions: [
          t('addSkill.error.suggestion.checkRepo'),
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };

    case 'gitNetworkError':
    case 'gitHubApiError':
      // GitHubApiError 主要在 update 流程出现 (检查 skill folder hash 时),
      // install 路径基本走不到。此处仅为 exhaustive match 兜底,统一归到 network 文案。
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
        details: t('addSkill.error.cloneTimeoutDetails', {
          timeout: error.data.timeoutSecs,
        }),
        suggestions: [
          t('addSkill.error.suggestion.adjustCloneTimeout'),
          t('addSkill.error.suggestion.checkNetwork'),
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

    case 'agentSelectionInvalid':
      return {
        message: t(`agentSelection.error.${error.data.reason}`),
        suggestions: [t('addSkill.error.suggestion.reselectAgents')],
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

    case 'installRiskConfirmationRequired':
      return {
        message: t('addSkill.error.riskConfirmationRequired'),
        details: error.data.code,
        suggestions: [
          t('addSkill.error.suggestion.reviewRiskAndConfirm'),
        ],
      };

    case 'mutationBusy':
      return {
        message: t('addSkill.error.mutationBusy'),
        suggestions: [
          t('addSkill.error.suggestion.retryOrContact'),
        ],
      };

    case 'installWizardActive':
      return {
        message: t('addSkill.error.installWizardActive'),
      };

    case 'installWizardSessionUnavailable':
      return {
        message: t('addSkill.error.installWizardSessionUnavailable'),
      };

    case 'applicationTerminating':
      return {
        message: t('addSkill.error.applicationTerminating'),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'wslIntegrationBusy':
      return {
        message: t('settings.general.wslBusyOperation'),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'mutationCancelled':
      return {
        message: t('addSkill.error.mutationCancelled'),
      };

    case 'environmentDiscoveryFailed':
      return {
        message: t('addSkill.error.environmentDiscoveryFailed'),
        details: error.data.message,
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'wslCommandTimedOut':
      return {
        message: t('addSkill.error.wslCommandTimedOut'),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'wslOutputLimitExceeded':
      return {
        message: t('addSkill.error.wslOutputLimitExceeded', {
          stream: error.data.stream,
          limit: error.data.limit,
        }),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'wslCommandFailed':
      return {
        message: t('addSkill.error.wslCommandFailed', {
          exitCode: error.data.exitCode ?? t('common.unknown'),
        }),
        details: error.data.stderr || undefined,
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'environmentUnavailable':
      return {
        message: t('addSkill.error.environmentUnavailable'),
        details: error.data.message,
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'storageMappingUnsupported':
      return {
        message: t('addSkill.error.storageMappingUnsupported', {
          path: error.data.path,
        }),
        suggestions: [t('addSkill.error.suggestion.chooseAccessiblePath')],
      };

    case 'projectMigrationFailed':
      return {
        message: error.data.message,
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'configurationReadOnly':
      return {
        message: t('addSkill.error.configurationReadOnly'),
        suggestions: [t('addSkill.error.suggestion.chooseWritableConfiguration')],
      };

    case 'payloadStorageRequiresCleanup':
      return {
        message: t('addSkill.error.payloadStorageRequiresCleanup'),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'validation':
      return {
        message: error.data.message,
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'environmentChanged':
    case 'contextChanged':
    case 'payloadSessionExpired':
    case 'staleContext':
    case 'staleRegistry':
    case 'staleEnvironment':
    case 'stalePayload':
    case 'staleTarget':
    case 'staleAgentRuntime':
      return {
        message: t('addSkill.error.staleState'),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'storageUnsupported':
    case 'unsafePath':
    case 'unsafeSourceLink':
      return {
        message: t('addSkill.error.storageUnsupported'),
        suggestions: [t('addSkill.error.suggestion.chooseAccessiblePath')],
      };
    case 'capabilityUnavailable':
      if (error.kind === 'capabilityUnavailable'
        && (error.data.capability === 'runtimeMaintenancePending'
          || error.data.capability === 'runtimeMaintenanceFailed')) {
        return {
          message: t('addSkill.error.runtimeMaintenance'),
          suggestions: [t('addSkill.error.suggestion.retryOrContact')],
        };
      }
      return {
        message: t('addSkill.error.storageUnsupported'),
        suggestions: [t('addSkill.error.suggestion.chooseAccessiblePath')],
      };

    case 'selfCopy':
      return { message: t('addSkill.error.selfCopy') };

    case 'externalLockChanged':
      return {
        message: t('addSkill.error.externalLockChanged'),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'executionFailed':
    case 'restoreFailed':
    case 'recoveryRequired':
    case 'configurationCorrupted':
      return {
        message: error.data.message,
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };

    case 'lockConflict': {
      const { target } = error.data;
      return {
        message: target.kind === 'skill'
          ? t('addSkill.error.lockConflict', { skill: target.skillName })
          : t('addSkill.error.agentDefaultsConflict'),
        suggestions: [t('addSkill.error.suggestion.retryOrContact')],
      };
    }

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
