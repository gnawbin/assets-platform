#!/usr/bin/env node

import fs from 'fs/promises';
import path from 'path';
import { execSync } from 'child_process';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, '..');

/**
 * 验证版本号格式 (semantic versioning)
 */
function isValidVersion(version) {
  const semverRegex = /^v?\d+\.\d+\.\d+(?:-[a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)*)?(?:\+[a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)*)?$/;
  return semverRegex.test(version);
}

/**
 * 提取纯版本号（去除v前缀）
 */
function extractPureVersion(version) {
  return version.startsWith('v') ? version.slice(1) : version;
}

/**
 * 更新 package.json
 */
async function updatePackageJson(newVersion) {
  const filePath = path.join(projectRoot, 'package.json');
  const content = await fs.readFile(filePath, 'utf8');
  const json = JSON.parse(content);
  const oldVersion = json.version;
  json.version = newVersion;
  await fs.writeFile(filePath, JSON.stringify(json, null, 2) + '\n');
  return oldVersion;
}

/**
 * 更新 Cargo.toml
 */
async function updateCargoToml(newVersion) {
  const filePath = path.join(projectRoot, 'src-tauri', 'Cargo.toml');
  const content = await fs.readFile(filePath, 'utf8');
  const match = content.match(/^version\s*=\s*"([^"]*)"$/m);
  const oldVersion = match ? match[1] : 'unknown';
  const updated = content.replace(/^(version\s*=\s*")[^"]*(")$/m, `$1${newVersion}$2`);
  await fs.writeFile(filePath, updated);
  return oldVersion;
}

/**
 * 更新 tauri.conf.json
 */
async function updateTauriConfig(newVersion) {
  const filePath = path.join(projectRoot, 'src-tauri', 'tauri.conf.json');
  const content = await fs.readFile(filePath, 'utf8');
  const json = JSON.parse(content);
  const oldVersion = json.version;
  json.version = newVersion;
  await fs.writeFile(filePath, JSON.stringify(json, null, 2) + '\n');
  return oldVersion;
}

/**
 * 更新 Sidebar.tsx 中的版本号显示
 */
async function updateSidebar(newVersion) {
  const filePath = path.join(projectRoot, 'src', 'components', 'Sidebar.tsx');
  const content = await fs.readFile(filePath, 'utf8');
  const match = content.match(/v\d+\.\d+\.\d+/);
  const oldVersion = match ? match[0] : 'unknown';
  const updated = content.replace(/v\d+\.\d+\.\d+/g, `v${newVersion}`);
  await fs.writeFile(filePath, updated);
  return oldVersion;
}

/**
 * 检查是否有未提交的更改
 */
function checkGitStatus() {
  try {
    const status = execSync('git status --porcelain', { cwd: projectRoot, encoding: 'utf8' }).trim();
    if (status) {
      console.log('⚠️  检测到未提交的更改:');
      console.log(status);
      console.log('建议先提交所有更改后再创建版本标签。');
    }
  } catch {
    console.log('⚠️  无法检查 git 状态');
  }
}

/**
 * 提交修改并推送到远端
 */
function commitAndPushChanges(version) {
  console.log('📝 添加修改的文件到暂存区...');
  execSync('git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/Cargo.lock src/components/Sidebar.tsx src-tauri/src/database/sql/public_initial_data.sql src-tauri/src/database/sql/knowledge_module_migration.sql', { cwd: projectRoot, encoding: 'utf8' });
  console.log('✅ 已添加修改的文件到暂存区');

  const commitMessage = `chore: bump version to v${version}`;
  console.log(`📝 创建提交: ${commitMessage}`);
  execSync(`git commit -m "${commitMessage}"`, { cwd: projectRoot, encoding: 'utf8' });
  console.log('✅ 已创建提交');

  console.log('📝 推送更改到远端...');
  execSync('git push', { cwd: projectRoot, encoding: 'utf8' });
  console.log('✅ 已推送更改到远端');
}

/**
 * 创建并推送 git 标签
 */
function createAndPushTag(version) {
  const tagName = `v${version}`;
  // 检查标签是否已存在
  try {
    execSync(`git rev-parse ${tagName}`, { cwd: projectRoot, stdio: 'pipe' });
    // 如果没抛异常说明标签已存在
    console.log(`⚠️  标签 ${tagName} 已存在，跳过创建`);
    return;
  } catch {
    // 标签不存在，继续创建
  }

  console.log(`📝 创建标签: ${tagName}`);
  execSync(`git tag -a ${tagName} -m "Release version ${version}"`, { cwd: projectRoot, encoding: 'utf8' });
  console.log(`✅ 已创建标签: ${tagName}`);

  console.log(`📝 推送标签到远端: ${tagName}`);
  execSync(`git push origin ${tagName}`, { cwd: projectRoot, encoding: 'utf8' });
  console.log(`✅ 已推送标签到远端: ${tagName}`);
}

/**
 * 主函数
 */
async function main() {
  try {
    const args = process.argv.slice(2);

    if (args.length === 0 || args[0] === '--help' || args[0] === '-h') {
      console.log('📦 版本升级脚本');
      console.log('\n用法: node script/update-version.js <version>');
      console.log('\n参数:');
      console.log('  <version>    新版本号（支持v前缀）');
      console.log('\n示例:');
      console.log('  node script/update-version.js v0.0.7');
      console.log('\n功能:');
      console.log('  - 更新 package.json');
      console.log('  - 更新 Cargo.toml');
      console.log('  - 更新 tauri.conf.json');
      console.log('  - 更新 Sidebar.tsx（版本显示）');
      console.log('  - 提交修改并推送');
      console.log('  - 创建并推送 Git 标签');
      process.exit(0);
    }

    const inputVersion = args[0];
    if (!isValidVersion(inputVersion)) {
      console.error('❌ 版本号格式无效');
      process.exit(1);
    }
    const newVersion = extractPureVersion(inputVersion);

    console.log(`🚀 开始更新项目版本到: v${newVersion}`);
    console.log('='.repeat(50));

    checkGitStatus();

    // 更新各个文件
    const oldPkg = await updatePackageJson(newVersion);
    console.log(`✅ 已更新 package.json: ${oldPkg} → ${newVersion}`);

    const oldCargo = await updateCargoToml(newVersion);
    console.log(`✅ 已更新 Cargo.toml: ${oldCargo} → ${newVersion}`);

    const oldTauri = await updateTauriConfig(newVersion);
    console.log(`✅ 已更新 tauri.conf.json: ${oldTauri} → ${newVersion}`);

    const oldSidebar = await updateSidebar(newVersion);
    console.log(`✅ 已更新 Sidebar.tsx: ${oldSidebar} → v${newVersion}`);

    console.log('\n📝 版本号更新完成，准备提交更改...');
    commitAndPushChanges(newVersion);

    console.log('\n📝 准备创建 git 标签...');
    createAndPushTag(newVersion);

    console.log('\n🎉 版本升级完成!');
    console.log(`   v${newVersion}`);
    console.log('\n✅ 更新的文件:');
    console.log('  - package.json');
    console.log('  - Cargo.toml');
    console.log('  - tauri.conf.json');
    console.log('  - Sidebar.tsx');

  } catch (error) {
    console.error(`\n❌ 版本升级失败: ${error.message}`);
    process.exit(1);
  }
}

main();