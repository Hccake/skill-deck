import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/** kebab-case → Title Case (e.g., "doc-skills" → "Doc Skills") */
export function toTitleCase(kebab: string): string {
  return kebab.split('-').map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(' ');
}

/**
 * 将 ISO 时间字符串格式化为用户友好的本地时间
 *
 * 根据时间距离当前的远近，显示不同格式：
 * - 1 分钟内：刚刚 / just now
 * - 1 小时内：X 分钟前 / X minutes ago
 * - 今天内：今天 HH:MM / Today HH:MM
 * - 昨天：昨天 HH:MM / Yesterday HH:MM
 * - 今年内：MM-DD HH:MM
 * - 更早：YYYY-MM-DD HH:MM
 */
export function formatTime(isoString: string | undefined, locale?: string): string {
  if (!isoString) return '';

  const date = new Date(isoString);
  if (isNaN(date.getTime())) return isoString;

  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMinutes = Math.floor(diffMs / 60000);

  const isZh = locale?.startsWith('zh');
  const timeStr = date.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit', hour12: false });

  // 1 分钟内
  if (diffMinutes < 1) {
    return isZh ? '刚刚' : 'just now';
  }

  // 1 小时内
  if (diffMinutes < 60) {
    return isZh
      ? `${diffMinutes} 分钟前`
      : `${diffMinutes} minute${diffMinutes === 1 ? '' : 's'} ago`;
  }

  // 判断日期
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today.getTime() - 86400000);
  const dateDay = new Date(date.getFullYear(), date.getMonth(), date.getDate());

  // 今天
  if (dateDay.getTime() === today.getTime()) {
    const label = isZh ? '今天' : 'Today';
    return `${label} ${timeStr}`;
  }

  // 昨天
  if (dateDay.getTime() === yesterday.getTime()) {
    const label = isZh ? '昨天' : 'Yesterday';
    return `${label} ${timeStr}`;
  }

  // 今年内
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');

  if (date.getFullYear() === now.getFullYear()) {
    return `${month}-${day} ${timeStr}`;
  }

  // 更早
  return `${date.getFullYear()}-${month}-${day} ${timeStr}`;
}
