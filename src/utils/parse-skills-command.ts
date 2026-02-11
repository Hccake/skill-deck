// src/utils/parse-skills-command.ts

/** CLI 命令解析结果 */
export interface ParsedCommand {
  /** 提取的 source（owner/repo 或 URL） */
  source: string;
  /** --skill / -s 指定的 skill 名称列表 */
  skills: string[];
  /** --agent / -a 指定的 agent ID 列表 */
  agents: string[];
  /** 是否识别为 CLI 命令 */
  isCommand: boolean;
}

/** 匹配 skills CLI 命令前缀 */
const COMMAND_PREFIX = /^(?:npx\s+)?skills\s+(?:add|install|a|i)\s+/i;

/** 不带值的 flag（遇到后直接跳过） */
const BOOLEAN_FLAGS = new Set([
  '-g', '--global',
  '-y', '--yes',
  '--all',
  '-l', '--list',
]);

/** 带值的 flag（遇到后消费下一个 token） */
const VALUE_FLAGS = new Set([
  '-s', '--skill',
  '-a', '--agent',
]);

/**
 * 将命令字符串拆分为 token 列表，正确处理单引号和双引号。
 *
 * 示例:
 *   tokenize('--skill "Convex Best Practices" -a claude-code')
 *   => ['--skill', 'Convex Best Practices', '-a', 'claude-code']
 */
function tokenize(input: string): string[] {
  const tokens: string[] = [];
  let current = '';
  let inSingle = false;
  let inDouble = false;

  for (let i = 0; i < input.length; i++) {
    const ch = input[i];

    if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
      continue;
    }
    if (ch === '"' && !inSingle) {
      inDouble = !inDouble;
      continue;
    }
    if (ch === ' ' && !inSingle && !inDouble) {
      if (current) {
        tokens.push(current);
        current = '';
      }
      continue;
    }
    current += ch;
  }

  if (current) {
    tokens.push(current);
  }

  return tokens;
}

/**
 * 解析 skills CLI 安装命令。
 *
 * 支持的格式:
 *   npx skills add <source> [--skill <name>]... [--agent <id>]... [-g] [-y] [--all]
 *   skills install <source> -s <name> -a <id>
 *
 * 如果输入不是 CLI 命令格式，返回 isCommand: false，source 为原始输入。
 */
export function parseSkillsCommand(input: string): ParsedCommand {
  const trimmed = input.trim();

  const match = COMMAND_PREFIX.exec(trimmed);
  if (!match) {
    return { source: trimmed, skills: [], agents: [], isCommand: false };
  }

  // 剥离前缀后 tokenize
  const rest = trimmed.slice(match[0].length);
  const tokens = tokenize(rest);

  let source = '';
  const skills: string[] = [];
  const agents: string[] = [];

  let i = 0;
  while (i < tokens.length) {
    const token = tokens[i];

    if (BOOLEAN_FLAGS.has(token)) {
      i++;
      continue;
    }

    if (VALUE_FLAGS.has(token)) {
      const value = tokens[i + 1];
      if (value !== undefined) {
        // '*' 通配符视为不预选
        if (value !== '*') {
          if (token === '-s' || token === '--skill') {
            skills.push(value);
          } else {
            agents.push(value);
          }
        }
        i += 2;
      } else {
        i++;
      }
      continue;
    }

    // 非 flag token：第一个作为 source，后续忽略
    if (!source) {
      source = token;
    }
    i++;
  }

  return { source, skills, agents, isCommand: true };
}
