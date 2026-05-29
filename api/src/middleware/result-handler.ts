import type { Response } from 'express';
import type { ApiErrorCode, ApiResult } from '../types/api-result';

const HTTP_STATUS: Record<ApiErrorCode, number> = {
  VALIDATION_ERROR: 400,
  UNAUTHORIZED: 401,
  FORBIDDEN: 403,
  NOT_FOUND: 404,
  CONFLICT: 409,
  SERVICE_UNAVAILABLE: 503,
  INTERNAL_ERROR: 500,
};

export function sendResult<T>(res: Response, result: ApiResult<T>, successStatus = 200): Response {
  if (result.ok) {
    return res.status(successStatus).json(result.value);
  }

  return res.status(HTTP_STATUS[result.error.code]).json({
    success: false,
    error: {
      code: result.error.code,
      message: result.error.message,
      context: result.error.context,
    },
  });
}
