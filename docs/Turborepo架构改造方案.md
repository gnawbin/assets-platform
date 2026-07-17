# Turborepo 架构改造方案

## 一、当前架构评估

### 现状分析

当前项目是一个单体仓库（Monorepo）中的**单体应用**，尚未使用 Turborepo。

| 检查项 | 结果 |
|--------|------|
| `turbo.json` 配置文件 | ❌ 不存在 |
| `package.json` 中声明 `workspaces` | ❌ 无 |
| `pnpm-workspace.yaml` 中声明 `packages` | ❌ 只有 `allowBuilds` 和 `minimumReleaseAgeExclude` |
| `apps/` 或 `packages/` 目录结构 | ❌ 不存在 |
| `turbo` 作为依赖 | ❌ 未安装 |
| 脚本使用 `turbo dev` / `turbo build` | ❌ 直接调用 `next` / `tauri` 命令 |

### 项目实际架构

```
Frontend (Next.js, src/)
        │
        ├── Tauri IPC ──→ Rust 后端 (src-tauri/)
        │                    ├── commands/    ← Tauri IPC 命令
        │                    ├── api/         ← HTTP RESTful API (axum, 端口 3001)
        │                    ├── service/     ← 业务逻辑层（共用）
        │                    └── database/    ← 数据库操作（SurrealDB + PostgreSQL）
        │
        └── HTTP API ───→ 同上 Rust 后端 (端口 3001)
```

> **注意：** `src-tauri/` 不仅是桌面壳，它实质上是**完整的 Rust 后端程序**：
> - 通过 Tauri IPC（`commands/`）为桌面模式提供接口
> - 通过 axum HTTP API（`api/`）为 Web 模式提供接口
> - Service 层和数据库层是**两者共用**的
> - 由环境变量 `adapter = "tauri" | "http"` 控制前端使用哪种通信方式

### 为什么需要 Turborepo

核心诉求：**管理多个子包 / 跨应用共享代码**

- 前端 `apps/web`（Next.js）和后端 `apps/backend`（Rust）需要独立管理
- 前后端共享的类型定义（资产模型、用户模型、API 接口定义）需要抽取到独立包
- 构建缓存加速（尤其是 Next.js 构建）

---

## 二、Turborepo + pnpm 职责分工

| | pnpm | Turborepo |
|---|---|---|
| **职责** | 包管理器 | 任务编排器 |
| **做什么** | 安装依赖、管理 `node_modules`、处理 workspace 链接 | 缓存构建产物、并行执行任务、编排 pipeline |
| **配置文件** | `pnpm-workspace.yaml` | `turbo.json` |
| **关键命令** | `pnpm install`、`pnpm add` | `turbo run dev`、`turbo run build` |

- Turborepo **不替代 pnpm**，两者是合作关系
- pnpm 管依赖，Turborepo 管任务编排
- `workspace:*` 协议仍然是 pnpm 管理本地包引用的方式

---

## 三、最终目录结构

```
assets-platform/
├── turbo.json                          # [新增] Turborepo pipeline 配置
├── package.json                        # [修改] 精简为 workspace 聚合 + 公共 scripts
├── pnpm-workspace.yaml                 # [修改] 添加 packages 定义
├── .gitignore                          # [修改] 适配新路径
├── .env.example.toml                   # [保留] 作为文档参考
├── docker-compose.yml                  # [保留] 不变
├── otel-collector-config.yaml          # [保留] 不变
├── README.md                           # [保留] 不变
├── TODO.md                             # [保留] 不变
├── script/                             # [保留] 不变
├── docs/                               # [保留] 不变
│
├── apps/
│   ├── web/                            # [新建] Next.js 前端（从根目录 src/ 迁移）
│   │   ├── src/                        # ← 当前根目录的 src/（整体移入）
│   │   │   ├── app/                    # Next.js App Router 页面
│   │   │   ├── assets/                 # 静态资源（react.svg 等）
│   │   │   ├── components/             # React 组件
│   │   │   ├── hooks/                  # 自定义 Hooks
│   │   │   ├── services/               # 前端服务层
│   │   │   ├── store/                  # 状态管理（zustand）
│   │   │   ├── tests/                  # 前端测试
│   │   │   ├── types/                  # TypeScript 类型定义
│   │   │   │   └── tauri.d.ts
│   │   │   └── utils/                  # 工具函数
│   │   ├── public/                     # ← 当前根目录的 public/（整体移入）
│   │   ├── .storybook/                 # ← 当前根目录的 .storybook/（移入）
│   │   ├── next.config.js              # [从根目录移入 + 修改路径别名]
│   │   ├── tsconfig.json               # [从根目录移入 + 添加 shared 路径映射]
│   │   ├── tsconfig.jest.json          # [从根目录移入]
│   │   ├── jest.config.js              # [从根目录移入 + 修改 moduleNameMapper]
│   │   ├── next-env.d.ts               # [从根目录移入]
│   │   ├── .env.local                  # [移入] 前端环境变量
│   │   └── package.json                # [新建] 继承根目录的依赖
│   │
│   └── backend/                        # [新建] Rust 后端
│       ├── src-tauri/                  # ← 当前 src-tauri/（整体移入）
│       │   ├── src/
│       │   │   ├── api/                # axum HTTP API 路由
│       │   │   ├── commands/           # Tauri IPC 命令
│       │   │   ├── database/           # 数据库操作
│       │   │   ├── engine/             # 引擎（Skill 等）
│       │   │   ├── service/            # 业务逻辑层
│       │   │   ├── storage/            # 文件存储
│       │   │   ├── utils/              # 工具函数
│       │   │   ├── workflow/           # 工作流
│       │   │   └── lib.rs              # [修改] load_env 路径适应
│       │   ├── Cargo.toml
│       │   ├── Cargo.lock
│       │   ├── tauri.conf.json         # [修改] frontendDist / beforeDevCommand 路径
│       │   ├── build.rs
│       │   ├── capabilities/
│       │   ├── gen/
│       │   └── icons/
│       ├── tests/                      # ← 当前 src-tauri/tests/（移入）
│       ├── .env.toml                   # [移入] 后端环境变量
│       └── package.json                # [新建] 仅含 tauri 相关 scripts
│
├── packages/
│   ├── shared/                         # [新建] 共享类型和工具
│   │   ├── src/
│   │   │   ├── index.ts
│   │   │   ├── types/
│   │   │   │   ├── asset.ts
│   │   │   │   ├── user.ts
│   │   │   │   ├── category.ts
│   │   │   │   └── ...
│   │   │   └── utils/
│   │   ├── tsconfig.json
│   │   └── package.json
│   │
│   └── ui/                             # [可选/渐进引入] 共享 UI 组件
│       ├── src/
│       ├── tsconfig.json
│       └── package.json
│
└── .env.example.toml                   # [保留] 不变
```

---

## 四、文件变更清单

### 4.1 根目录文件变更

#### 4.1.1 `pnpm-workspace.yaml`（修改）

```yaml
# 保留现有配置
allowBuilds:
  core-js-pure: false
  esbuild: false
  sharp: false
  unrs-resolver: false
minimumReleaseAgeExclude:
  - '@types/node@26.1.0'

# 新增：定义 workspace 包路径
packages:
  - 'apps/*'
  - 'packages/*'
```

#### 4.1.2 `package.json`（根目录 — 修改）

所有运行时依赖和 devDependencies（除 `turbo`、`prettier` 外）迁移到 `apps/web/package.json`。

```json
{
  "name": "assets-platform",
  "private": true,
  "version": "0.0.8",
  "type": "module",
  "scripts": {
    "dev": "turbo run dev",
    "dev:web": "turbo run dev --filter=@assets/web...",
    "dev:backend": "turbo run dev --filter=@assets/backend...",
    "build": "turbo run build",
    "build:web": "turbo run build --filter=@assets/web...",
    "build:backend": "turbo run build --filter=@assets/backend...",
    "test": "turbo run test",
    "lint": "turbo run lint",
    "clean": "turbo run clean",
    "format": "prettier --write \"**/*.{ts,tsx,js,json,css,md}\"",
    "storybook": "cd apps/web && pnpm storybook",
    "build-storybook": "cd apps/web && pnpm build-storybook",
    "tauri-dev": "cd apps/backend && pnpm tauri-dev",
    "tauri-build": "cd apps/backend && pnpm tauri-build",
    "setup": "node script/install-deps.cjs"
  },
  "dependencies": {},
  "devDependencies": {
    "turbo": "^2.5.0",
    "prettier": "^3.6.2"
  },
  "packageManager": "pnpm@11.13.0+sha512..."
}
```

**变更要点：**
- 移除所有运行时依赖 → 移到 `apps/web/package.json`
- 只保留 `turbo` 和 `prettier` 作为根 devDependencies
- scripts 改为通过 `turbo run` 编排
- `tauri-dev` 和 `tauri-build` 通过 `cd apps/backend && pnpm ...` 交给 backend 子包管理

#### 4.1.3 `turbo.json`（新增）

```json
{
  "$schema": "https://turbo.build/schema.json",
  "globalDependencies": ["**/.env.*local"],
  "pipeline": {
    "build": {
      "dependsOn": ["^build"],
      "outputs": [".next/**", "dist/**", "!.next/cache/**"]
    },
    "dev": {
      "cache": false,
      "persistent": true
    },
    "test": {
      "dependsOn": ["^build"],
      "outputs": []
    },
    "lint": {
      "outputs": []
    },
    "clean": {
      "cache": false
    }
  }
}
```

**pipeline 说明：**
- `build`：先构建依赖（`^build`），输出 `.next/` 和 `dist/`
- `dev`：不缓存（因为 dev server 是持久进程），并行启动
- `test`：先构建再测试
- `clean`：不缓存

#### 4.1.4 `.gitignore`（修改）

```gitignore
# 在现有 gitignore 基础上添加：
apps/*/node_modules
apps/*/.next
apps/*/dist
apps/backend/src-tauri/target

# 移除 pnpm-workspace.yaml 的忽略（需要提交）
# pnpm-workspace.yaml  ← 删除这一行
```

---

### 4.2 `apps/web/` 文件

#### 4.2.1 `apps/web/package.json`（新建）

```json
{
  "name": "@assets/web",
  "version": "0.0.8",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "next dev --webpack -p 1480",
    "build": "next build --webpack",
    "preview": "next start",
    "test": "jest",
    "test:watch": "jest --watch",
    "test:coverage": "jest --coverage",
    "lint": "next lint",
    "storybook": "storybook dev -p 6006",
    "build-storybook": "storybook build",
    "clean": "rm -rf .next dist coverage"
  },
  "dependencies": {
    "@mantine/charts": "^9.4.1",
    "@mantine/code-highlight": "^9.4.1",
    "@mantine/core": "^9.4.1",
    "@mantine/dates": "^9.4.1",
    "@mantine/dropzone": "^9.4.1",
    "@mantine/form": "^9.4.1",
    "@mantine/hooks": "^9.4.1",
    "@mantine/modals": "^9.4.1",
    "@mantine/notifications": "^9.4.1",
    "@mantine/nprogress": "^9.4.1",
    "@mantine/spotlight": "^9.4.1",
    "@mantine/tiptap": "^9.4.1",
    "@mdxeditor/editor": "^4.0.4",
    "@next/bundle-analyzer": "^16.2.10",
    "@opentelemetry/api": "^1.9.1",
    "@opentelemetry/api-logs": "^0.220.0",
    "@opentelemetry/context-zone": "^2.7.1",
    "@opentelemetry/exporter-logs-otlp-http": "^0.220.0",
    "@opentelemetry/exporter-metrics-otlp-http": "^0.220.0",
    "@opentelemetry/exporter-trace-otlp-http": "^0.220.0",
    "@opentelemetry/instrumentation": "^0.220.0",
    "@opentelemetry/instrumentation-document-load": "^0.65.0",
    "@opentelemetry/instrumentation-fetch": "^0.220.0",
    "@opentelemetry/instrumentation-xml-http-request": "^0.220.0",
    "@opentelemetry/resources": "^2.7.1",
    "@opentelemetry/sdk-logs": "^0.220.0",
    "@opentelemetry/sdk-metrics": "^2.7.1",
    "@opentelemetry/sdk-trace-base": "^2.7.1",
    "@opentelemetry/sdk-trace-web": "^2.7.1",
    "@opentelemetry/semantic-conventions": "^1.41.1",
    "@puckeditor/core": "^0.22.0",
    "@tabler/icons-react": "^3.44.0",
    "@tauri-apps/api": "2.11.0",
    "@tauri-apps/plugin-clipboard-manager": "^2.3.2",
    "@tauri-apps/plugin-dialog": "^2.7.1",
    "@tauri-apps/plugin-http": "~2.5.9",
    "@tauri-apps/plugin-opener": "^2.5.4",
    "@tauri-apps/plugin-shell": "^2.3.5",
    "@tauri-apps/plugin-store": "~2.4.3",
    "@tiptap/extension-link": "^3.26.0",
    "@tiptap/pm": "^3.26.0",
    "@tiptap/react": "^3.26.0",
    "@tiptap/starter-kit": "^3.11.0",
    "cytoscape": "^3.34.0",
    "dayjs": "^1.11.21",
    "embla-carousel": "^8.6.0",
    "embla-carousel-react": "^8.6.0",
    "next": "^16.2.10",
    "qrcode": "^1.5.4",
    "react": "^19.2.7",
    "react-dom": "^19.2.7",
    "react-markdown": "^10.1.0",
    "xlsx": "^0.18.5",
    "zod": "4.4.3",
    "zustand": "^5.0.12",
    "@assets/shared": "workspace:*"
  },
  "devDependencies": {
    "@babel/core": "^8.0.1",
    "@eslint/eslintrc": "^3.3.5",
    "@eslint/js": "^10.0.1",
    "@ianvs/prettier-plugin-sort-imports": "^4.7.0",
    "@playwright/test": "^1.54.1",
    "@storybook/addon-essentials": "^8.6.14",
    "@storybook/addon-interactions": "^8.6.14",
    "@storybook/addon-themes": "^10.4.3",
    "@storybook/nextjs": "^10.4.3",
    "@storybook/react": "^10.4.3",
    "@tauri-apps/cli": "2.11.0",
    "@testing-library/dom": "^10.4.1",
    "@testing-library/jest-dom": "^6.9.1",
    "@testing-library/react": "^16.3.0",
    "@testing-library/user-event": "^14.6.1",
    "@types/cytoscape": "^3.21.9",
    "@types/eslint-plugin-jsx-a11y": "^6",
    "@types/jest": "^30.0.0",
    "@types/node": "^26.1.0",
    "@types/react": "19.2.17",
    "@types/react-dom": "^19.2.3",
    "echarts": "*",
    "eslint": "^10.4.1",
    "eslint-config-mantine": "^4.0.3",
    "eslint-config-next": "16.2.9",
    "eslint-plugin-jsx-a11y": "^6.10.2",
    "eslint-plugin-react": "^7.37.5",
    "fake-indexeddb": "^6.2.5",
    "highlight.js": "^11.11.1",
    "jest": "^30.2.0",
    "jest-environment-jsdom": "^30.2.0",
    "postcss": "^8.5.6",
    "postcss-preset-mantine": "1.18.0",
    "postcss-simple-vars": "^7.0.1",
    "prettier": "^3.6.2",
    "sharp": "^0.35.0",
    "storybook": "^10.5.0",
    "stylelint": "^17.13.0",
    "stylelint-config-standard-scss": "^17.0.0",
    "terser": "^5.43.1",
    "ts-jest": "^29.4.4",
    "typescript": "7.0.2",
    "typescript-eslint": "^8.46.0"
  }
}
```

#### 4.2.2 `apps/web/next.config.js`（从根目录移入 + 修改）

```js
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  output: "export",
  distDir: "dist",
  images: {
    unoptimized: true,
  },
  webpack: (config) => {
    config.resolve.fallback = {
      fs: false,
      net: false,
      tls: false,
      crypto: false,
      stream: false,
      buffer: false,
    };
    config.resolve.alias = {
      ...config.resolve.alias,
      '@': path.resolve(__dirname, 'src'),
      '@assets/shared': path.resolve(__dirname, '../../packages/shared/src'),
    };
    return config;
  },
};

export default nextConfig;
```

**变更要点：** 添加 `@assets/shared` 别名指向 `packages/shared/src`

#### 4.2.3 `apps/web/tsconfig.json`（从根目录移入 + 修改）

```json
{
  "compilerOptions": {
    "types": ["node", "jest", "@testing-library/jest-dom"],
    "allowJs": true,
    "skipLibCheck": true,
    "strict": true,
    "noEmit": true,
    "incremental": true,
    "module": "esnext",
    "esModuleInterop": true,
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "jsx": "react-jsx",
    "paths": {
      "@/*": ["./src/*"],
      "@assets/shared": ["../../packages/shared/src"],
      "@assets/shared/*": ["../../packages/shared/src/*"]
    },
    "target": "ES2022",
    "lib": ["dom", "dom.iterable", "esnext"],
    "plugins": [
      { "name": "next" }
    ]
  },
  "include": [
    "**/*.ts",
    "**/*.tsx",
    "next-env.d.ts",
    "dist/types/**/*.ts",
    "dist/dev/types/**/*.ts",
    ".next/types/**/*.ts",
    ".next/dev/types/**/*.ts"
  ],
  "exclude": [
    "node_modules",
    ".next",
    "dist",
    "../../src-tauri/target/**/*"
  ]
}
```

#### 4.2.4 `apps/web/jest.config.js`（从根目录移入 + 修改）

```js
/** @type {import('jest').Config} */
const config = {
  preset: 'ts-jest',
  testEnvironment: 'jsdom',
  setupFilesAfterEnv: ['<rootDir>/src/tests/setup.ts'],
  moduleNameMapper: {
    '^@/(.*)$': '<rootDir>/src/$1',
    '^@assets/shared/(.*)$': '<rootDir>/../../packages/shared/src/$1',
  },
  testMatch: [
    '<rootDir>/src/tests/**/*.test.ts',
    '<rootDir>/src/tests/**/*.test.tsx',
  ],
  collectCoverageFrom: [
    'src/**/*.{ts,tsx}',
    '!src/**/*.d.ts',
    '!src/tests/**/*',
  ],
  coverageDirectory: 'coverage',
  coverageReporters: ['text', 'lcov', 'html'],
  transform: {
    '^.+\\.(ts|tsx)$': ['ts-jest', {
      tsconfig: 'tsconfig.jest.json',
    }],
  },
};

export default config;
```

---

### 4.3 `apps/backend/` 文件

#### 4.3.1 `apps/backend/package.json`（新建）

```json
{
  "name": "@assets/backend",
  "version": "0.0.8",
  "private": true,
  "scripts": {
    "dev": "cd src-tauri && cargo tauri dev",
    "build": "cd src-tauri && cargo tauri build",
    "test": "cd src-tauri && cargo test",
    "lint": "cd src-tauri && cargo clippy",
    "clean": "cd src-tauri && cargo clean"
  }
}
```

**说明：** Rust/Cargo 不依赖 pnpm 管理，`package.json` 仅为 Turborepo 编排而设，提供 npm scripts 入口。

#### 4.3.2 `apps/backend/src-tauri/tauri.conf.json`（修改）

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "资产管理平台",
  "version": "0.0.7",
  "identifier": "com.it.assets",
  "build": {
    "beforeDevCommand": "cd ../../web && pnpm dev",
    "devUrl": "http://localhost:1480",
    "beforeBuildCommand": "cd ../../web && pnpm build",
    "frontendDist": "../../web/dist"
  },
  "app": {
    "windows": [
      {
        "title": "资产管理平台",
        "width": 1280,
        "height": 800
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

**变更要点：** 路径从 `../dist` 改为 `../../web/dist`，`beforeDevCommand` 改为 `cd ../../web && pnpm dev`

#### 4.3.3 `src-tauri/src/lib.rs`（修改 `load_env` 路径适应）

```rust
fn load_env() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // 现在 manifest_dir 是 apps/backend/src-tauri/
    // root_dir = apps/backend/
    let root_dir = manifest_dir.parent().unwrap_or(manifest_dir);
    
    // 尝试多个路径查找 .env.toml 文件
    let content = std::fs::read_to_string(".env.toml")
        .or_else(|_| std::fs::read_to_string("src-tauri/.env.toml"))  // 备用：在 src-tauri 内
        .or_else(|_| std::fs::read_to_string(&root_dir.join(".env.toml"))) // apps/backend/.env.toml
        .or_else(|_| {
            // 找到项目根目录（需要向上走两级：apps/backend/ → apps/ → 项目根）
            let project_root = root_dir.parent()
                .and_then(|p| p.parent())
                .map(|p| p.join(".env.toml"));
            match project_root {
                Some(path) => std::fs::read_to_string(path),
                None => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "")),
            }
        });
    // ...
}
```

---

### 4.4 `packages/shared/` 文件

#### 4.4.1 `packages/shared/package.json`（新建）

```json
{
  "name": "@assets/shared",
  "version": "0.0.8",
  "private": true,
  "type": "module",
  "main": "./src/index.ts",
  "types": "./src/index.ts",
  "scripts": {
    "lint": "tsc --noEmit",
    "clean": "rm -rf node_modules"
  },
  "devDependencies": {
    "typescript": "^7.0.2"
  }
}
```

#### 4.4.2 `packages/shared/tsconfig.json`（新建）

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "esnext",
    "moduleResolution": "bundler",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "declaration": true,
    "declarationMap": true,
    "sourceMap": true,
    "outDir": "./dist",
    "rootDir": "./src"
  },
  "include": ["src"]
}
```

#### 4.4.3 `packages/shared/src/index.ts`（示例）

```typescript
// 这个文件作为 shared 包的入口
// 后续可在此导出共享类型定义

export * from './types/asset';
export * from './types/user';
export * from './types/category';
```

---

## 五、Rust 路径兼容性注意事项

`lib.rs` 中的 `load_env()` 函数需要适配新目录结构：

```
改造前:
  CARGO_MANIFEST_DIR = /project/assets-platform/src-tauri/
  root_dir = /project/assets-platform/
  
改造后:
  CARGO_MANIFEST_DIR = /project/assets-platform/apps/backend/src-tauri/
  root_dir = /project/assets-platform/apps/backend/
  项目根 = /project/assets-platform/
```

需要额外向上走两级找到项目根目录。

.env.toml 查找优先级：
1. `apps/backend/src-tauri/.env.toml`（Cargo 工作目录）
2. `apps/backend/.env.toml`
3. 项目根 `.env.toml`

---

## 六、实施步骤

建议按以下顺序逐步改造，**每一步都可验证、可回滚**。

### 阶段一：最小改动（安全可逆）

| 步骤 | 操作 | 验证方式 | 风险 |
|------|------|----------|------|
| 1 | 创建新分支 `feat/turborepo` | `git branch` | 低 |
| 2 | 根目录安装 `turbo`：`pnpm add -D turbo` | `pnpm turbo --version` | 低 |
| 3 | 创建 `turbo.json` | 文件存在 | 低 |
| 4 | 修改 `pnpm-workspace.yaml` 添加 `packages` | 文件格式正确 | 低 |
| 5 | 执行 `pnpm install` 验证 | 无报错 | 中 |
| 6 | 测试 `turbo run build` 能正常构建 | 构建成功 | 中 |

### 阶段二：拆分 apps/

| 步骤 | 操作 | 验证方式 | 风险 |
|------|------|----------|------|
| 7 | 创建 `apps/web/` 目录结构 | 目录存在 | 低 |
| 8 | 复制 `src/`、`public/`、`.storybook/` 等到 `apps/web/` | 文件完整 | 低 |
| 9 | 复制 `next.config.js`、`tsconfig.json`、`jest.config.js`、`tsconfig.jest.json`、`next-env.d.ts` 到 `apps/web/` | 文件完整 | 低 |
| 10 | 创建 `apps/web/package.json`（含所有依赖） | | 低 |
| 11 | 创建 `apps/backend/` 目录 | 目录存在 | 低 |
| 12 | 复制 `src-tauri/`、`tests/`、`.env.toml` 到 `apps/backend/` | 文件完整 | 低 |
| 13 | 创建 `apps/backend/package.json` | | 低 |
| 14 | 修改 `tauri.conf.json` 路径 | | 低 |
| 15 | 修改 `lib.rs` 中 `load_env` 路径 | | 低 |
| 16 | 执行 `pnpm install` | 无报错 | 中 |
| 17 | 测试 `turbo run dev --filter=@assets/web` 能启动 Next.js | `localhost:1480` 可访问 | 中 |
| 18 | 测试 `pnpm tauri-dev` 能启动 Tauri（先手动启动 web dev） | 桌面窗口打开 | 中 |
| 19 | 确认一切正常后，删除根目录的 `src/`、`src-tauri/`、`next.config.js` 等源文件 | | 高 |
| 20 | 更新 `.gitignore` | | 低 |

### 阶段三：共享包

| 步骤 | 操作 | 风险 |
|------|------|------|
| 21 | 创建 `packages/shared/` 目录和配置 | 低 |
| 22 | 从前端定义中抽取共享类型（资产、用户、分类等） | 低 |
| 23 | 在 `apps/web/package.json` 中添加 `@assets/shared: workspace:*` | 低 |
| 24 | 修改前端代码引用，从 `@assets/shared` 导入 | 中 |
| 25 | 验证类型检查和构建正常 | 中 |

---

## 七、注意事项

### 7.1 `tauri dev` 兼容性

改造后 `tauri dev` 仍然可用，只是路径配置做了同步调整：

- `tauri.conf.json` 中的 `frontendDist` 从 `../dist` 改为 `../../web/dist`
- `beforeDevCommand` 改为 `cd ../../web && pnpm dev`
- 或者开发者可以先手动 `cd apps/web && pnpm dev`，再 `cd apps/backend && pnpm tauri-dev`

### 7.2 环境变量文件

- `.env.toml`（后端配置）放在 `apps/backend/` 下
- `.env.local`（前端配置）放在 `apps/web/` 下
- 根目录 `.env.example.toml` 作为文档参考保留

### 7.3 构建输出目录

- Next.js 构建输出：`apps/web/dist/`
- Cargo 构建输出：`apps/backend/src-tauri/target/`
- `.gitignore` 需要相应更新

### 7.4 持续集成（CI/CD）

改造后需要更新 CI/CD 配置：

```yaml
# 示例：GitHub Actions
jobs:
  build:
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
      - run: pnpm install
      - run: pnpm turbo run build
        env:
          TURBO_TOKEN: ${{ secrets.TURBO_TOKEN }}
          TURBO_TEAM: ${{ vars.TURBO_TEAM }}
```

---

## 八、渐进式策略（推荐）

不一定要一步到位，建议分三个阶段推进，每个阶段都有明确的验证点：

```
阶段一（最小改动）
  ┌─────────────┐     阶段二（拆分 apps）        阶段三（共享包）
  │ 只创建 turbo │     ┌──────────────────┐     ┌──────────────────┐
  │ + 修改 yaml  │ ──→ │ 拆分 web/backend │ ──→ │ 创建 packages/   │
  │ 不移动文件   │     │ 调整路径配置     │     │ 抽取共享类型     │
  │ 验证缓存     │     │ 验证 dev/build   │     │ 验证类型安全     │
  └─────────────┘     └──────────────────┘     └──────────────────┘
```

- **阶段一**：1-2 小时，验证 Turborepo 缓存和 pipeline 机制
- **阶段二**：1-2 天，核心改造工作
- **阶段三**：持续进行，按需抽取共享代码