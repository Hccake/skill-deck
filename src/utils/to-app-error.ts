import type { AppError } from '@/bindings';

/**
 * 将 catch 块中的 unknown error 转换为 AppError
 * unwrap() 抛出的是 AppError 对象，但 TypeScript 的 catch 类型是 unknown
 */
export function toAppError(error: unknown): AppError {
  if (error != null && typeof error === 'object' && 'kind' in error) {
    return error as AppError;
  }
  return {
    kind: 'custom',
    data: { message: error instanceof Error ? error.message : String(error) },
  };
}
