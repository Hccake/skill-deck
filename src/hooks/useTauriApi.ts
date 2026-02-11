// src/hooks/useTauriApi.ts
import { invoke } from '@tauri-apps/api/core';
import type { Agent, ListSkillsResult, SkillScope, RemoveResult } from '@/types';

/** list_skills 参数 */
interface ListSkillsParams {
  scope?: SkillScope;
  projectPath?: string;
}

/**
 * 列出所有 Agents（包括未安装的）
 * 返回完整信息供前端使用，前端无需额外计算
 * 调用 Rust: list_agents command
 */
export async function listAgents(): Promise<Agent[]> {
  return invoke<Agent[]>('list_agents');
}

/**
 * 列出已安装的 Skills
 * 调用 Rust: list_skills command
 */
export async function listSkills(params?: ListSkillsParams): Promise<ListSkillsResult> {
  return invoke<ListSkillsResult>('list_skills', { params: params ?? {} });
}

// ============ 配置相关 API ============

/** Skill Deck 配置 */
export interface SkillDeckConfig {
  projects: string[];
}

/**
 * 获取应用配置
 * 调用 Rust: get_config command
 */
export async function getConfig(): Promise<SkillDeckConfig> {
  return invoke<SkillDeckConfig>('get_config');
}

/**
 * 保存应用配置
 * 调用 Rust: save_config command
 */
export async function saveConfig(config: SkillDeckConfig): Promise<void> {
  return invoke<void>('save_config', { config });
}

// ============ Agent 选择相关 API ============

/**
 * 获取上次选择的 agents
 * 读取 ~/.agents/.skill-lock.json 中的 lastSelectedAgents
 */
export async function getLastSelectedAgents(): Promise<string[]> {
  return invoke<string[]>('get_last_selected_agents');
}

/**
 * 保存选择的 agents
 * 写入 ~/.agents/.skill-lock.json 中的 lastSelectedAgents
 */
export async function saveLastSelectedAgents(agents: string[]): Promise<void> {
  return invoke<void>('save_last_selected_agents', { agents });
}

// ============ 安装相关 API ============

import type {
  FetchResult,
  InstallParams,
  InstallResults,
} from '@/components/skills/add-skill/types';

/**
 * 从来源获取可用的 skills 列表
 * 调用 Rust: fetch_available command
 */
export async function fetchAvailable(source: string): Promise<FetchResult> {
  return invoke<FetchResult>('fetch_available', { source });
}

/**
 * 安装选中的 skills
 * 调用 Rust: install_skills command
 */
export async function installSkills(params: InstallParams): Promise<InstallResults> {
  return invoke<InstallResults>('install_skills', { params });
}

/**
 * 检测覆盖情况
 * 调用 Rust: check_overwrites command
 */
export async function checkOverwrites(
  skills: string[],
  agents: string[],
  scope: 'global' | 'project',
  projectPath?: string
): Promise<Record<string, string[]>> {
  return invoke<Record<string, string[]>>('check_overwrites', {
    skills,
    agents,
    scope,
    projectPath,
  });
}

// ============ 删除相关 API ============

/**
 * 删除指定 skill
 * 调用 Rust: remove_skill command
 */
export async function removeSkill(params: {
  scope: SkillScope;
  name: string;
  projectPath?: string;
}): Promise<RemoveResult> {
  return invoke<RemoveResult>('remove_skill', params);
}

// ============ 项目管理 API ============

/**
 * 添加项目路径
 * 已存在则忽略，返回更新后的 projects 列表
 */
export async function addProject(path: string): Promise<string[]> {
  return invoke<string[]>('add_project', { path });
}

/**
 * 移除项目路径
 * 返回更新后的 projects 列表
 */
export async function removeProject(path: string): Promise<string[]> {
  return invoke<string[]>('remove_project', { path });
}

/**
 * 检查项目路径是否存在
 */
export async function checkProjectPath(path: string): Promise<boolean> {
  return invoke<boolean>('check_project_path', { path });
}

/**
 * 在系统文件管理器中打开路径
 */
export async function openInExplorer(path: string): Promise<void> {
  return invoke<void>('open_in_explorer', { path });
}
