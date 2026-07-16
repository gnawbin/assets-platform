/**
 * 前端日志工具
 *
 * 封装统一的日志 API，同时输出到：
 * - 控制台（开发环境）
 * - OpenTelemetry（生产环境，通过 OTLP HTTP 导出）
 *
 * 自动附加当前用户信息、页面路径等上下文。
 *
 * 使用方式：
 * ```ts
 * import { logger } from '@/utils/logger';
 * logger.info('用户登录成功', { userId: 1 });
 * logger.error('操作失败', { error: new Error('...') });
 * ```
 */

import type { Logger } from '@opentelemetry/api-logs';

// ======================== 日志级别 ========================

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

// ======================== 日志上下文 ========================

export interface LogContext {
  /** 当前页面路径 */
  page?: string;
  /** 用户 ID */
  userId?: number;
  /** 用户名 */
  username?: string;
  /** 模块名称 */
  module?: string;
  /** 操作名称 */
  action?: string;
  /** 其他自定义属性 */
  [key: string]: unknown;
}

// ======================== 日志条目 ========================

interface LogEntry {
  timestamp: string;
  level: LogLevel;
  message: string;
  context?: LogContext;
  error?: {
    name: string;
    message: string;
    stack?: string;
  };
}

// ======================== 日志工具类 ========================

class LoggerService {
  private otelLogger: Logger | null = null;
  private initialized = false;

  /**
   * 初始化日志服务
   * @param otelLogger OpenTelemetry Logger 实例
   */
  init(otelLogger: Logger): void {
    this.otelLogger = otelLogger;
    this.initialized = true;
  }

  /**
   * 获取当前页面路径
   */
  private getPagePath(): string {
    if (typeof window !== 'undefined') {
      return window.location.pathname;
    }
    return '';
  }

  /**
   * 获取当前用户信息
   */
  private getUserInfo(): { userId?: number; username?: string } {
    try {
      if (typeof window !== 'undefined') {
        const stored = localStorage.getItem('auth_user');
        if (stored) {
          const user = JSON.parse(stored);
          return {
            userId: user.id,
            username: user.username,
          };
        }
      }
    } catch {
      // 静默失败
    }
    return {};
  }

  /**
   * 创建日志条目
   */
  private createLogEntry(
    level: LogLevel,
    message: string,
    context?: LogContext,
    error?: Error,
  ): LogEntry {
    const entry: LogEntry = {
      timestamp: new Date().toISOString(),
      level,
      message,
      context: {
        page: this.getPagePath(),
        ...this.getUserInfo(),
        ...context,
      },
    };

    if (error) {
      entry.error = {
        name: error.name,
        message: error.message,
        stack: error.stack,
      };
    }

    return entry;
  }

  /**
   * 输出日志到控制台
   */
  private consoleLog(entry: LogEntry): void {
    const { level, message, context, error } = entry;
    const prefix = `[${level.toUpperCase()}]`;

    switch (level) {
      case 'debug':
        console.debug(prefix, message, context || '', error || '');
        break;
      case 'info':
        console.info(prefix, message, context || '', error || '');
        break;
      case 'warn':
        console.warn(prefix, message, context || '', error || '');
        break;
      case 'error':
        console.error(prefix, message, context || '', error || '');
        break;
    }
  }

  /**
   * 输出日志到 OpenTelemetry
   */
  private otelLog(entry: LogEntry): void {
    if (!this.otelLogger || !this.initialized) return;

    try {
      const severityNumber = this.getSeverityNumber(entry.level);
      const attributes: Record<string, string | number | boolean | undefined> = {};

      // 将 context 展平为 attributes
      if (entry.context) {
        for (const [key, value] of Object.entries(entry.context)) {
          if (value !== undefined && value !== null) {
            if (typeof value === 'object') {
              attributes[key] = JSON.stringify(value);
            } else {
              attributes[key] = value as string | number | boolean;
            }
          }
        }
      }

      // 添加错误信息
      if (entry.error) {
        attributes['error.name'] = entry.error.name;
        attributes['error.message'] = entry.error.message;
        if (entry.error.stack) {
          attributes['error.stack'] = entry.error.stack;
        }
      }

      this.otelLogger.emit({
        severityNumber,
        severityText: entry.level.toUpperCase(),
        body: entry.message,
        attributes,
        timestamp: new Date(entry.timestamp).getTime(),
      });
    } catch (e) {
      // OTel 日志失败时回退到控制台
      console.error('[Logger] OTel 日志导出失败:', e);
    }
  }

  /**
   * 获取 OTel 严重级别数字
   */
  private getSeverityNumber(level: LogLevel): number {
    switch (level) {
      case 'debug':
        return 5; // SEVERITY_DEBUG
      case 'info':
        return 9; // SEVERITY_INFO
      case 'warn':
        return 13; // SEVERITY_WARN
      case 'error':
        return 17; // SEVERITY_ERROR
    }
  }

  // ======================== 公开 API ========================

  debug(message: string, context?: LogContext): void {
    const entry = this.createLogEntry('debug', message, context);
    this.consoleLog(entry);
    this.otelLog(entry);
  }

  info(message: string, context?: LogContext): void {
    const entry = this.createLogEntry('info', message, context);
    this.consoleLog(entry);
    this.otelLog(entry);
  }

  warn(message: string, context?: LogContext): void {
    const entry = this.createLogEntry('warn', message, context);
    this.consoleLog(entry);
    this.otelLog(entry);
  }

  error(message: string, error?: Error, context?: LogContext): void {
    const entry = this.createLogEntry('error', message, context, error);
    this.consoleLog(entry);
    this.otelLog(entry);
  }
}

// ======================== 导出单例 ========================

/** 全局日志实例 */
export const logger = new LoggerService();
