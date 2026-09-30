# Skill Deck 领域词汇

Skill Deck 管理 AI Agent 使用的 Skill，以及这些 Skill 在不同 Environment 和 Scope 中的安装关系。

## AI Agent 与 Skill

**AI Agent**：
能够根据用户目标决定行动步骤、调用工具，并根据执行结果调整后续行动的 AI 智能体。  
_Avoid_: AI 编程 Agent

**Agent Skill（技能，以下简称 Skill）**：
一种可复用的 AI Agent 能力，汇集了处理特定任务所需的知识和工作方法。AI Agent 在处理相关任务时按需加载它；Skill 还可以附带脚本、模板和参考资料。  
_Avoid_: 任务能力包

**Agent Skills 规范**：
规定 Skill 如何声明用途、编写使用指引和组织附带资源，使兼容 Agent 能够发现并加载 Skill 的开放规范。

**`SKILL.md`**：
Skill 的主说明文件，包含名称、用途等元数据，以及 Agent 使用该 Skill 时遵循的指引。

**`skills` CLI**：
由 `vercel-labs/skills` 项目独立维护的第三方 Skill 管理工具。  
_Avoid_: 上游 CLI、Skill Deck CLI

**Skill 来源（Source）**：
Skill 内容的提供位置，例如 Git 仓库、本地目录或 Well-known 地址。一个来源可以包含一个或多个 Skill。

**Well-known 来源**：
通过约定地址提供 Skill 索引、制品声明和版本证据的 Skill 来源。

**直接下载来源（Download Source）**：
直接提供一个 `SKILL.md` 文件或 Skill 归档的网络来源，不提供 Well-known 索引。
_Avoid_: 直接 URL 来源、制品来源

**直接安装 Skill（已安装 Skill）**：
直接安装到某个 Scope、由该 Scope 维护的一份 Skill，可以位于通用目录或 Agent 专用目录。界面中的“已安装 Skill”指这类安装对象，与通过库应用提供的 Skill 相区分。
_Avoid_: 直接 Skill

**Skill 库（Skill Library）**：
用户在一个 Environment 中创建并独立维护的 Skill 集合。同一个 Skill 可以分别加入多个库，各库保存自己的内容和来源记录。

**库成员**：
某个 Skill 库独立保存和维护的一份 Skill，其身份由所属库和 Skill 名称共同确定。

**退役成员**：
已经从库的可用成员集合中移除、为现有引用或恢复需要而暂时保留内容的库成员。

**Skill 库应用关系**：
某个 Scope 与一组有序 Skill 库之间的使用关系。

## Scope 与 Skill 目录

**全局 Skill（Global Skill）**：
安装在某个 Environment 的全局位置、不属于任何具体 Project 的 Skill。  
_Avoid_: 用户级 Skill、系统级 Skill

**项目 Skill（Project Skill）**：
安装在某个 Project 中、属于该 Project 的 Skill。

**Scope**：
当前 Environment 中的全局管理范围或某个项目管理范围。一个 Scope 包含通用 Skill 目录，以及该范围内各 Agent 的 Agent 专用 Skill 目录。
_Avoid_: Context（仅在指代 Skill 管理范围时）、Context Scope、Skill 范围

**操作位置**：
一次 Skill 操作固定使用的 Environment 和 Scope。
_Avoid_: 系统位置

**Skill 目录**：
Scope 内实际保存或链接 Skill 的目录，分为通用 Skill 目录和 Agent 专用 Skill 目录。不同路径可能指向同一个物理目录。

**通用 Skill 目录**：
Agent Skills 生态为跨 Agent 共享 Skill 而广泛采用的 `.agents/skills` 目录。它可以位于当前用户的主目录中，也可以位于具体 Project 中。  
_Avoid_: 共享 Skill 目录、共用 Skill 目录

**Agent 专用 Skill 目录**：
某个 Agent 按自身约定读取 Skill 的目录，区别于面向跨 Agent 共享的通用 Skill 目录。

**直接安装项**：
Agent 专用 Skill 目录中提供直接安装 Skill 内容的目录或链接。  
_Avoid_: Agent 专用安装项、Agent Skill 安装项、Agent 目录项

**版本选举**：
针对一个 Scope 中的同名 Skill，确定每个物理目录使用直接安装版本、库版本或不保留该 Skill 的过程。

## Environment 与 Project

**Environment**：
Skill Deck 用来区分 Skill、Project 和文件操作归属的管理环境。它可以是桌面应用所在的 Windows、macOS 或 Linux，也可以是 Windows 中某个 WSL 发行版提供的 Linux 环境。  
_Avoid_: Host、OS（仅在指代 Environment 时）

**Native Environment**：
Skill Deck 桌面应用实际运行的操作系统。  
_Avoid_: Host Environment、本机 Host

**WSL Environment**：
Windows 中某个具名 WSL 发行版提供的 Linux 环境。每个发行版构成一个独立的 Environment。

**Environment 本地存储**：
归属于一个 Environment 用户空间的持久化位置，与其他 Environment 的数据相互独立。

**Project**：
AI Agent 开展工作的目录，通常也是代码仓库的根目录。

**已添加项目（Registered Project）**：
用户已经加入 Skill Deck 管理的 Project。  
_Avoid_: ProjectBinding、项目绑定

## Agent 读取与关联

**Skill 读取位置**：
某个 Agent 查找并加载 Skill 时会扫描的目录。Skill Deck 分别记录该 Agent 在全局和具体 Project 中使用的读取位置。

**Agent 检测位置（Detection Location）**：
Skill Deck 用于判断某个外部 AI Agent 是否已经安装的文件或目录位置。

**关联 Agent**：
在当前操作位置能够读取某个 Skill 实际版本的 AI Agent。关联可能来自该 Skill 的直接安装关系（直接关联），也可能来自库应用中的 Agent 选择（库关联）。

## 恢复

**恢复记录**：
Skill Deck 为一次无法确认写入结果是否一致的 Skill 操作保留的待处理条目。  
_Avoid_: 恢复资源

**恢复数据**：
与恢复记录关联、用于检查和处理对应写入结果的状态标记、备份和一致性证据。
