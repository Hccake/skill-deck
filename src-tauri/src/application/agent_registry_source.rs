use std::sync::Arc;

use crate::core::agent_registry::AgentRegistrySnapshot;

/// Application 用例取得当前 Agent 注册表快照的 Interface。具体实现由持有注册表状态的一方提供。
pub trait AgentRegistrySnapshotSource: Send + Sync {
    fn snapshot(&self) -> Arc<AgentRegistrySnapshot>;
}
