---
status: accepted
---

# 连接 WSL 时允许按需唤起发行版

## 背景

用户选择已经安装但当前停止的 WSL 发行版时，WSL CLI 没有可靠的“执行但禁止启动”模式。读取前先检查运行状态会增加进程调用，仍无法消除检查与实际连接之间的状态变化。

## 决定

Skill Deck 允许连接和一次受控重连按需唤起已经停止的发行版，但不提供启动、停止、重启、安装或注销 WSL 的独立功能。

## 理由与重新讨论条件

连接需要能够真正建立可用 Environment，额外的预检查既不能保证结果，也会把 WSL 生命周期管理扩大为产品职责。只有 WSL 提供可靠的无副作用连接探测，或者 Skill Deck 明确扩展为 WSL 生命周期管理工具时，才重新讨论这一决定。

当前行为由[Environment 与 Context](../environments-and-contexts.md#environment)和[产品设计](../product.md#context-侧栏)负责说明。
