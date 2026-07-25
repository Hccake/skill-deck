---
status: accepted
---

# Recovery Resource 只保护受保护写入的一致性证据

Skill Deck 只为已经进入可能改变 Skill 目录、Agent 目录项或关联 lock 的受保护写入建立持久化 Recovery Resource；Source 获取、Environment 连接、Runtime Maintenance、普通配置保存和临时 Payload 清理使用各自的失败或 warning 语义。Recovery Resource 归属于产生它的 Environment 和 storage owner，产品只保证保留受控证据、重新评估以及在用户确认且状态一致后清理，不承诺自动或手动恢复一定成功，也不续跑旧操作。这样可以保护 destructive write 的数据安全，同时避免把所有异常扩展成 Recovery 产品和全局阻断。
