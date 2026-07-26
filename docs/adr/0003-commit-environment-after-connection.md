---
status: accepted
---

# Environment 连接成功后独立加载项目

## 背景

切换 Environment 时，连接本身与项目注册信息读取可能分别成功或失败。如果要求二者全部成功才切换，项目配置损坏会把用户留在旧 Environment，也会阻断目标 Environment 的 Global Context 和相关修复入口。

## 决定

Environment 连接成功后立即提交切换结果，并先进入目标 Global Context。项目注册信息随后独立加载，以加载中、可用或失败状态呈现；加载失败不撤销已经成功的 Environment 切换。

## 理由

项目列表是 Environment 内的一项附属数据。连接成功后先进入 Global Context，可以保留目标 Environment 的基础能力和项目配置修复入口。

## 重新讨论条件

未来所有 Environment 能力都依赖完整项目注册表，并且无法提供 Global 降级路径时，可以重新评估连接与项目加载是否需要合并为一个事务。

当前行为由[Environment 与 Context](../environments-and-contexts.md#context)和[产品设计](../product.md#context-侧栏)负责说明。
