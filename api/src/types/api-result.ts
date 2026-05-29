export type ApiErrorCode =
  | 'VALIDATION_ERROR'
  | 'UNAUTHORIZED'
  | 'FORBIDDEN'
  | 'NOT_FOUND'
  | 'CONFLICT'
  | 'SERVICE_UNAVAILABLE'
  | 'INTERNAL_ERROR';

export interface ApiError {
  code: ApiErrorCode;
  message: string;
  context?: Record<string, unknown>;
}

export type ApiResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: ApiError };

export const ok = <T>(value: T): ApiResult<T> => ({ ok: true, value });

export const fail = (
  code: ApiErrorCode,
  message: string,
  context?: Record<string, unknown>,
): ApiResult<never> => ({ ok: false, error: { code, message, context } });
