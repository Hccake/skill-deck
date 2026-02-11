// src/types/index.ts

/** Agent 类型（与 Rust AgentType 对应） */
export type AgentType =
  | 'amp'
  | 'antigravity'
  | 'augment'
  | 'claude-code'
  | 'openclaw'
  | 'cline'
  | 'codebuddy'
  | 'codex'
  | 'command-code'
  | 'continue'
  | 'crush'
  | 'cursor'
  | 'droid'
  | 'gemini-cli'
  | 'github-copilot'
  | 'goose'
  | 'iflow-cli'
  | 'junie'
  | 'kilo'
  | 'kimi-cli'
  | 'kiro-cli'
  | 'kode'
  | 'mcpjam'
  | 'mistral-vibe'
  | 'mux'
  | 'neovate'
  | 'opencode'
  | 'openhands'
  | 'pi'
  | 'qoder'
  | 'qwen-code'
  | 'replit'
  | 'roo'
  | 'trae'
  | 'trae-cn'
  | 'windsurf'
  | 'zencoder'
  | 'pochi'
  | 'adal';

/**
 * Agent 信息（来自 list_agents）
 * 对应 Rust: AgentInfo
 * 对应 CLI: AgentConfig + detectInstalled
 */
export interface Agent {
  id: AgentType;
  name: string;
  skillsDir: string;
  globalSkillsDir: string;
  detected: boolean;
  /** 是否是 Universal Agent（安装逻辑用） */
  isUniversal: boolean;
  /** 是否在 Universal 列表显示（UI 显示用） */
  showInUniversalList: boolean;
}


/** Skill 范围 */
export type SkillScope = 'global' | 'project';

/** 已安装的 Skill 信息（来自 list_skills） */
export interface Skill {
  name: string;
  description: string;
  path: string;
  canonicalPath: string;
  scope: SkillScope;
  agents: AgentType[];
  source?: string;
  sourceUrl?: string;
  installedAt?: string;
  updatedAt?: string;
  hasUpdate?: boolean;
}

/** 删除结果（对应 Rust RemoveResult，serde camelCase） */
export interface RemoveResult {
  skillName: string;
  success: boolean;
  removedPaths: string[];
  source?: string;
  sourceType?: string;
  error?: string;
}

/** list_skills 返回结果 */
export interface ListSkillsResult {
  skills: Skill[];
  /** 项目目录是否存在（project scope 时有意义，global 始终为 true） */
  pathExists: boolean;
}
