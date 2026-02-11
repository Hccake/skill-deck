import type { AvailableSkill, InstallError } from '@/components/skills/add-skill/types';

interface ErrorContext {
  selectedSkills?: string[];
  availableSkills?: AvailableSkill[];
}

/**
 * 解析安装错误，返回用户友好的错误信息
 */
export function parseInstallError(
  errorMessage: string,
  context: ErrorContext = {}
): InstallError {
  const { selectedSkills = [], availableSkills = [] } = context;

  // 1. Skill 不匹配
  if (errorMessage.includes('No skills matched') || errorMessage.includes('NoSkillsFound')) {
    const availableNames = availableSkills.map(s => s.name);
    return {
      message: '未找到匹配的 skills',
      details: selectedSkills.length > 0 && availableNames.length > 0
        ? `选择的 skills: ${selectedSkills.join(', ')}\n可用的 skills: ${availableNames.join(', ')}`
        : undefined,
      suggestions: [
        '检查 skill 名称是否正确',
        '返回上一步重新选择 skills',
      ],
    };
  }

  // 2. 网络错误
  if (
    errorMessage.includes('network') ||
    errorMessage.includes('clone failed') ||
    errorMessage.includes('Failed to clone') ||
    errorMessage.includes('Network')
  ) {
    return {
      message: '网络连接失败',
      details: errorMessage,
      suggestions: [
        '检查网络连接是否正常',
        '确认仓库地址是否正确',
        '如果是私有仓库，请配置 SSH 密钥或访问令牌',
      ],
    };
  }

  // 3. 权限错误
  if (errorMessage.includes('permission') || errorMessage.includes('Permission denied')) {
    return {
      message: '权限不足',
      details: errorMessage,
      suggestions: [
        '尝试使用管理员权限运行应用',
        '检查目标目录的写入权限',
      ],
    };
  }

  // 4. 仓库未找到
  if (errorMessage.includes('not found') || errorMessage.includes('404')) {
    return {
      message: '仓库未找到',
      details: errorMessage,
      suggestions: [
        '检查仓库地址是否正确',
        '确认仓库是否公开或你是否有访问权限',
      ],
    };
  }

  // 5. 默认：通用错误
  return {
    message: '安装失败',
    details: errorMessage,
    suggestions: ['请检查错误信息并重试', '如果问题持续，请查看日志或联系支持'],
  };
}
