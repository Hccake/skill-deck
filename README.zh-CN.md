<div align="center">
  <!-- TODO: 添加 Logo -->
  <!-- <img src="docs/images/logo.svg" alt="Skill Deck Logo" width="120"> -->
  <h1>Skill Deck</h1>
  <p>
    <strong>可与 skills CLI 配合使用的 Skill 管理桌面应用</strong>
  </p>

  <p>
    <img src="https://img.shields.io/badge/Tauri-v2-blue" alt="Tauri v2">
    <img src="https://img.shields.io/badge/React-19-61dafb" alt="React 19">
    <img src="https://img.shields.io/badge/skills%20CLI-compatible-green" alt="skills CLI compatible">
  </p>

  <a href="README.md">English</a>
</div>

---

Skill Deck 是一款跨平台桌面应用，用于浏览、安装、阅读、更新、复制和移除 AI Agent 使用的 Skill，也可以管理哪些 Agent 能够读取这些 Skill。

[`skills` CLI](https://github.com/vercel-labs/skills) 是由 `vercel-labs/skills` 项目独立维护的第三方工具。Skill Deck 可以读写该工具使用的 Skill 目录和兼容 lock 数据。两个工具独立运行，用户可以单独使用 Skill Deck，也可以根据需要搭配使用。

主要特点：

- **跨平台桌面应用**：支持 Windows、macOS 和 Linux；Windows 用户还可以按需使用已安装的 WSL 发行版。
- **可与 skills CLI 配合使用**：两个工具可以分别读写同一 Skill 目录和兼容 lock 数据。
- **完整 Skill 工作流**：提供安装、更新、来源修复、跨项目复制、Agent 关联管理和应用内更新。

---

## 界面预览

<p align="center">
  <img src="docs/images/skill_selected.png" alt="Skill 详情视图" width="900">
</p>
<p align="center"><em>浏览已安装的 Skill，查看完整内容，并检查或执行更新。</em></p>

<table>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/skills.png" alt="Skills 工作台">
      <br />
      <em>分别查看全局 Skill 与项目 Skill，并按 Agent 筛选。</em>
    </td>
    <td width="50%" align="center">
      <img src="docs/images/discover.png" alt="Discover 页面">
      <br />
      <em>浏览可安装的 Skill，并查看来源、说明和安全信息。</em>
    </td>
  </tr>
</table>

<table>
  <tr>
    <td width="50%" align="center">
      <img src="docs/images/agent_manage.png" alt="管理 Agent">
      <br />
      <em>调整哪些 Agent 能够读取已安装的 Skill。</em>
    </td>
    <td width="50%" align="center">
      <img src="docs/images/copy.png" alt="复制到其他项目">
      <br />
      <em>将项目 Skill 复制到当前或其他 Environment 中的项目。</em>
    </td>
  </tr>
</table>

---

## ✨ 功能

- 🗂 **Skills 工作台**：集中浏览已安装的 Skill，并查看内容、来源、更新状态和关联 Agent
- 🧠 **Agent 筛选与关联**：按 Agent 筛选 Skill，并调整哪些 Agent 能够读取已安装的 Skill
- 🌍 **全局与项目**：分别管理当前 Environment 中的全局 Skill 和各个项目的项目 Skill
- 🐧 **可选 WSL Environment**：Windows 用户可以在 Windows 与已安装的 WSL 发行版之间切换
- 🔍 **发现与安装**：支持 GitHub、Git、本地路径、Well-known 地址和 `skills add` 命令
- ♻️ **更新与来源修复**：检查可用更新，并在来源记录失效时重新选择来源
- 📋 **跨项目复制**：将项目 Skill 复制到一个或多个目标项目
- 📦 **两种安装方式**：按目标选择符号链接或文件复制
- 🧩 **补充 Agent 信息**：为尚未收录的 Agent 添加 Skill 读取位置和 Agent 检测位置
- 🌐 **中英文界面**：主窗口与安装向导可以切换简体中文和英语
- 🔄 **应用内更新**：检查、下载并安装 GitHub Release 中的新版本

Skill 是否可用取决于它的安装位置以及 Agent 是否读取该位置。

---

## 📦 安装

### 方式一：下载预编译包（推荐）

从 [GitHub Releases](https://github.com/hccake/skill-deck/releases) 下载对应平台的安装包：

- **Windows**：`skill-deck_x.x.x_windows_x64-setup.exe` 或 `skill-deck_x.x.x_windows_x64.msi`
- **macOS Apple Silicon**：`skill-deck_x.x.x_macos_aarch64.dmg`
- **macOS Intel**：`skill-deck_x.x.x_macos_x64.dmg`
  > macOS 构建目前没有 Apple 开发者签名。如果安装后被系统拦截，可执行：
  > ```bash
  > sudo xattr -rd com.apple.quarantine "/Applications/Skill Deck.app"
  > ```
- **Linux**：`skill-deck_x.x.x_linux_amd64.deb`、`skill-deck_x.x.x_linux_x86_64.rpm` 或 `skill-deck_x.x.x_linux_x86_64.AppImage`

### 方式二：从源码构建

开发环境和准确版本要求见[贡献指南](./CONTRIBUTING.md#开发环境)。Node.js、pnpm 和 Rust 版本分别以 CI、`package.json` 和 `src-tauri/Cargo.toml` 中的当前配置为准。

```bash
# 克隆仓库
git clone https://github.com/hccake/skill-deck.git
cd skill-deck

# 安装依赖
pnpm install --frozen-lockfile

# 启动桌面开发模式
pnpm tauri dev

# 构建生产版本
pnpm tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`。

---

## 🚀 快速开始

### 1. 添加项目

在侧栏的“项目”区域点击添加按钮，选择需要管理的代码项目目录。管理全局 Skill 时可以跳过此步骤。

### 2. 准备 Skill 来源

复制需要安装的 Skill 来源，例如：

- `https://github.com/vercel-labs/skills`
- `vercel-labs/skills`（GitHub 简写）
- `/path/to/local/skill`（本地路径）

也可以粘贴受支持的 `skills` CLI 安装命令。Skill Deck 会解析其中的来源、Skill 和目标 Agent：

```bash
npx skills add vercel-labs/agent-skills --skill frontend-design -a claude-code
```

### 3. 安装 Skill

在“全局 Skill”或目标项目区域点击新增入口，输入来源并选择 Skill、目标 Agent 和安装方式，然后确认预览并执行安装。粘贴 CLI 命令时，向导会预选命令中的 `--skill` 和 `--agent` 参数，确认前仍可调整。

### 4. 在 Agent 中使用

安装完成后，对应 Agent 会从通用 Skill 目录或 Agent 专用 Skill 目录读取内容。实际读取位置取决于 Skill Deck 记录的读取信息和本次安装选择。

---

## 📄 许可证

[Apache License 2.0](LICENSE)

---

## 🙏 致谢

- [vercel-labs/skills](https://github.com/vercel-labs/skills) — 被广泛使用的第三方 Skill 管理工具，也是 Skill Deck 的兼容与产品参考
- [Tauri](https://tauri.app/) — 跨平台桌面应用框架
- [Linux.do](https://linux.do/) — 社区支持与反馈
