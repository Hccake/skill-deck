---
status: accepted
---

# WSL 连接可按需唤起发行版

## 背景

用户选择已经安装但当前停止的 WSL 发行版时，`wsl.exe` 没有可靠的“执行但禁止启动”模式。读取前先检查运行状态会增加进程调用，仍无法消除检查与实际连接之间的状态变化。

## 决定

连接流程和一次受控重连可以按需唤起已经安装但处于停止状态的发行版。Windows 及其系统工具继续负责 WSL 的安装和生命周期管理，Skill Deck 负责发现、连接和执行 Skill 操作。

## 理由

连接的目标是建立可用 Environment。额外预检查无法消除检查与连接之间的状态变化，还会把 WSL 生命周期管理扩大为产品职责。

## 重新讨论条件

WSL 提供可靠且无副作用的连接探测，或者 Skill Deck 的产品范围明确扩展到 WSL 生命周期管理时，可以重新评估连接行为。

当前行为由[Environment 与 Context](../environments-and-contexts.md#environment)和[产品设计](../product.md#context-侧栏)负责说明。
