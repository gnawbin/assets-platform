/**
 * 前端 OpenTelemetry 初始化模块
 *
 * 基于 @opentelemetry/* 官方 JS SDK，通过 OTLP HTTP 协议
 * 将 traces（链路追踪）、logs（日志）、metrics（指标）发送到 OTel Collector。
 *
 * 与后端 Rust 的 tracing + opentelemetry 共享相同的 service.name，
 * 实现前后端统一的可观测性体系。
 *
 * 环境变量配置（通过 NEXT_PUBLIC_* 暴露给前端）：
 * - NEXT_PUBLIC_OTEL_ENABLED: 是否启用 OTel（默认 true）
 * - NEXT_PUBLIC_OTEL_EXPORTER_OTLP_ENDPOINT: OTLP HTTP 端点（默认 http://localhost:4318）
 * - NEXT_PUBLIC_OTEL_SERVICE_NAME: 服务名称（默认 assets-platform）
 */

import { diag, DiagConsoleLogger, DiagLogLevel } from '@opentelemetry/api';
import { OTLPTraceExporter } from '@opentelemetry/exporter-trace-otlp-http';
import { OTLPLogExporter } from '@opentelemetry/exporter-logs-otlp-http';
import { OTLPMetricExporter } from '@opentelemetry/exporter-metrics-otlp-http';
import { DocumentLoadInstrumentation } from '@opentelemetry/instrumentation-document-load';
import { FetchInstrumentation } from '@opentelemetry/instrumentation-fetch';
import { XMLHttpRequestInstrumentation } from '@opentelemetry/instrumentation-xml-http-request';
import { registerInstrumentations } from '@opentelemetry/instrumentation';
import { LoggerProvider, SimpleLogRecordProcessor } from '@opentelemetry/sdk-logs';
import { MeterProvider, PeriodicExportingMetricReader } from '@opentelemetry/sdk-metrics';
// @ts-expect-error - resourceFromAttributes exists at runtime but type defs may not export it
import { resourceFromAttributes } from '@opentelemetry/resources';
import {
  SEMRESATTRS_SERVICE_NAME,
  SEMRESATTRS_SERVICE_VERSION,
  SEMRESATTRS_DEPLOYMENT_ENVIRONMENT,
} from '@opentelemetry/semantic-conventions';
import { BatchSpanProcessor, WebTracerProvider } from '@opentelemetry/sdk-trace-web';
import { ZoneContextManager } from '@opentelemetry/context-zone';
import { logger } from './logger';

// ======================== 环境变量常量 ========================

/** 是否启用 OpenTelemetry 导出（默认 true） */
const ENV_OTEL_ENABLED = 'NEXT_PUBLIC_OTEL_ENABLED';
/** OTLP HTTP 端点（默认 http://localhost:4318） */
const ENV_OTEL_ENDPOINT = 'NEXT_PUBLIC_OTEL_EXPORTER_OTLP_ENDPOINT';
/** 服务名称（默认 assets-platform） */
const ENV_OTEL_SERVICE_NAME = 'NEXT_PUBLIC_OTEL_SERVICE_NAME';

// ======================== 默认值 ========================

const DEFAULT_ENDPOINT = 'http://localhost:4318';
const DEFAULT_SERVICE_NAME = 'assets-platform';

// ======================== 工具函数 ========================

/** 检查 OTel 是否启用 */
function isOtelEnabled(): boolean {
  const val = getEnv(ENV_OTEL_ENABLED);
  if (val !== undefined) {
    return val === '1' || val.toLowerCase() === 'true';
  }
  // 无法读取环境变量时，默认禁用（保守策略）
  return false;
}

/** 获取 OTLP 端点 */
function getOtelEndpoint(): string {
  if (typeof process !== 'undefined' && process.env?.[ENV_OTEL_ENDPOINT]) {
    return process.env[ENV_OTEL_ENDPOINT]!;
  }
  return DEFAULT_ENDPOINT;
}

/** 获取服务名称 */
function getServiceName(): string {
  if (typeof process !== 'undefined' && process.env?.[ENV_OTEL_SERVICE_NAME]) {
    return process.env[ENV_OTEL_SERVICE_NAME]!;
  }
  return DEFAULT_SERVICE_NAME;
}

/** 获取浏览器信息 */
function getBrowserInfo(): Record<string, string> {
  if (typeof navigator === 'undefined') return {};
  return {
    'browser.user_agent': navigator.userAgent,
    'browser.language': navigator.language,
    'browser.platform': navigator.platform,
  };
}

/** 转义字符串中的正则特殊字符 */
function escapeRegex(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** 安全获取环境变量（兼容浏览器端运行时） */
function getEnv(key: string): string | undefined {
  if (typeof process !== 'undefined' && process.env) {
    return (process.env as Record<string, string | undefined>)[key];
  }
  // 在浏览器端，NEXT_PUBLIC_* 变量在构建时已被内联替换
  // 通过全局 __NEXT_DATA__ 或直接访问 process.env（如果存在）
  return undefined;
}

/** 获取当前环境：'production' | 'development' | 'test' */
function getNodeEnv(): string {
  // 优先从 process.env 获取（构建时内联）
  const env = getEnv('NODE_ENV');
  if (env) return env;
  // 浏览器端回退：检查 hostname
  if (typeof location !== 'undefined') {
    return location.hostname === 'localhost' || location.hostname === '127.0.0.1'
      ? 'development'
      : 'production';
  }
  return 'development';
}

// ======================== 资源创建 ========================

/** 创建 OTel 资源 */
function createResource() {
  const attributes: Record<string, string> = {
    [SEMRESATTRS_SERVICE_NAME]: getServiceName(),
    [SEMRESATTRS_SERVICE_VERSION]: getEnv('NEXT_PUBLIC_APP_VERSION') || '0.0.2',
    [SEMRESATTRS_DEPLOYMENT_ENVIRONMENT]:
      getNodeEnv() === 'production' ? 'production' : 'development',
    ...getBrowserInfo(),
  };

  return resourceFromAttributes(attributes);
}

// ======================== 初始化函数 ========================

/**
 * 初始化前端 OpenTelemetry
 *
 * 应在应用最早阶段调用（layout.tsx 的 useEffect 中）。
 * 初始化内容包括：
 * - TracerProvider（链路追踪）
 * - LoggerProvider（日志）
 * - MeterProvider（指标）
 * - 自动检测插件（页面加载、fetch、XHR）
 */
export function initTelemetry(): void {
  // 检查是否启用
  if (!isOtelEnabled()) {
    console.info('[OTel] OpenTelemetry 已禁用');
    return;
  }

  // 仅在浏览器环境中初始化
  if (typeof window === 'undefined') {
    return;
  }

  // 防止重复初始化
  if ((window as any).__OTEL_INITIALIZED__) {
    return;
  }
  (window as any).__OTEL_INITIALIZED__ = true;

  // 开发环境启用详细日志
  if (getNodeEnv() !== 'production') {
    diag.setLogger(new DiagConsoleLogger(), DiagLogLevel.DEBUG);
  }

  const endpoint = getOtelEndpoint();
  const resource = createResource();

  console.info(`[OTel] 初始化 OpenTelemetry，端点: ${endpoint}`);

  try {
    // ==================== 1. 初始化 TracerProvider（链路追踪） ====================
    const traceExporter = new OTLPTraceExporter({
      url: `${endpoint}/v1/traces`,
    });

    const tracerProvider = new WebTracerProvider({
      resource,
      spanProcessors: [new BatchSpanProcessor(traceExporter)],
    });

    // 设置 ZoneContextManager 以支持异步上下文传播
    tracerProvider.register({
      contextManager: new ZoneContextManager(),
    });

    // ==================== 2. 初始化 LoggerProvider（日志） ====================
    const logExporter = new OTLPLogExporter({
      url: `${endpoint}/v1/logs`,
    });

    const loggerProvider = new LoggerProvider({
      resource,
      processors: [new SimpleLogRecordProcessor(logExporter)],
    } as any);

    // ==================== 3. 初始化 MeterProvider（指标） ====================
    const metricExporter = new OTLPMetricExporter({
      url: `${endpoint}/v1/metrics`,
    });

    const meterProvider = new MeterProvider({
      resource,
      readers: [
        new PeriodicExportingMetricReader({
          exporter: metricExporter,
          exportIntervalMillis: 60000, // 每分钟导出一次
        }),
      ],
    });

    // ==================== 4. 注册自动检测插件 ====================
    registerInstrumentations({
      tracerProvider,
      instrumentations: [
        new DocumentLoadInstrumentation(),
        new FetchInstrumentation({
          ignoreUrls: [/localhost:4318/, /127.0.0.1:4318/],
          propagateTraceHeaderCorsUrls: [new RegExp(escapeRegex(endpoint.replace(/\/$/, '')))],
        }),
        new XMLHttpRequestInstrumentation({
          ignoreUrls: [/localhost:4318/, /127.0.0.1:4318/],
        }),
      ],
    });

    // 保存 providers 以便后续使用
    (window as any).__OTEL_TRACER_PROVIDER__ = tracerProvider;
    (window as any).__OTEL_LOGGER_PROVIDER__ = loggerProvider;
    (window as any).__OTEL_METER_PROVIDER__ = meterProvider;

    // 初始化 LoggerService 的 OTel Logger 实例（修复 Bug 3）
    logger.init(loggerProvider.getLogger('assets-platform'));

    console.info('[OTel] OpenTelemetry 初始化完成');
  } catch (error) {
    console.error('[OTel] OpenTelemetry 初始化失败:', error);
  }
}

// ======================== 关闭函数 ========================

/**
 * 关闭 OpenTelemetry，确保所有数据被导出
 *
 * 应在应用退出时调用
 */
export async function shutdownTelemetry(): Promise<void> {
  const tracerProvider = (window as any).__OTEL_TRACER_PROVIDER__ as WebTracerProvider | undefined;
  const loggerProvider = (window as any).__OTEL_LOGGER_PROVIDER__ as LoggerProvider | undefined;
  const meterProvider = (window as any).__OTEL_METER_PROVIDER__ as MeterProvider | undefined;

  if (tracerProvider) {
    await tracerProvider.shutdown();
  }
  if (loggerProvider) {
    await loggerProvider.shutdown();
  }
  if (meterProvider) {
    await meterProvider.shutdown();
  }

  console.info('[OTel] OpenTelemetry 已关闭');
}
