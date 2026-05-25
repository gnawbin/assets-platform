/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // 静态导出模式（Tauri 需要）
  output: "export",
  // 导出目录（与 tauri.conf.json 中的 frontendDist 保持一致）
  distDir: "dist",
  images: {
    unoptimized: true,
  },
  // Tauri 兼容
  webpack: (config) => {
    config.resolve.fallback = {
      fs: false,
      net: false,
      tls: false,
      crypto: false,
      stream: false,
      buffer: false,
    };
    return config;
  },
};

export default nextConfig;
