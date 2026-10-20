<!--
  Licensed to the Apache Software Foundation (ASF) under one
  or more contributor license agreements.  See the NOTICE file
  distributed with this work for additional information
  regarding copyright ownership.  The ASF licenses this file
  to you under the Apache License, Version 2.0 (the
  "License"); you may not use this file except in compliance
  with the License.  You may obtain a copy of the License at

      http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing,
  software distributed under the License is distributed on an
  "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
  KIND, either express or implied.  See the License for the
  specific language governing permissions and limitations
  under the License.
-->

# maka-event-log

[English](README.md)

管理 SQLite 事件存储和 State Root 租约，持久化应用配置与包、操作回执、会话、轮次及恢复记录。独占所有权见 `root`，已提交的组合见 `applications`，中断恢复见 `recovery`。服务激活前必须先取得 Root 的持久化所有权；业务策略属于插件，规范事实由这里记录。

原始操作契约按摘要只存储一次。操作请求和回执引用只携带该身份，日志读取、调用方历史与 UI 传递不再逐次复制服务 schema。读取仍核对原始请求指纹，执行准入仍校验完整契约和资源要求。

消息交付保留准备后的内容、可编辑原文和最初提交的摘要。内部记录保存这些不同事实，编码后可能大于公开请求；写入与精确消息读取不再对整条编码记录重复套用请求大小限制。待处理队列不设固定条数或总字节额度。停止和整队撤回在同一事务的回执中保留完整获准条目，不另设回执大小额度。精确重试和重开后的读取返回同一份原始回执。通过 `editable_message` 按 Session、Turn 和消息身份读取一条已拥有的来源。

来源消息、执行器工具调用与生成的工具结果共享 Session 的可见消息身份空间。无论哪一方先声明，准入都依据规范事实检查冲突，包括执行器完成或调用终止时为待完成工具补出的结果。只有实际结果身份被占用；仅后缀相同不会预留身份。

读取 checkpoint 时会校验每个原始来源承诺，包括被最新摘要隐藏的前驱。前缀摘要依次绑定作用域、有序的序号／字节长度／原始字节记录，最后绑定 high-water；相同选择范围的多个边界可共用一次流式扫描。继承链选择和归档请求边界仍分别校验。该内部编码不保留旧格式兼容路径。

`run_source` 校验一个物理 Run 的全部原始事件，仅保留开头、末尾事件及精确边界。其摘要绑定 Run 内事件条数、原始账本位置和字节。原始流式片段不占用选定重放上下文的预算；handoff 仍在封口前为后继运行及立即取消预留空间。恢复校验沿已认证的来源逐步追溯到原始 Message；所有者读取沿显式后继查找当前执行。两者都拒绝循环，并保留任意深度的冻结继承关系。

存储打开期间，同一选择范围可复用已验证的 checkpoint 链，并延伸已认证的前缀。写连接的实时确认将证明绑定到读连接的固定快照或已取得写锁的事务；受保护历史的变更、外部提交、schema 变化及写入失败会撤销失效证明。选择范围、准入、结算和归档仍实时校验。仅含 opening 的读取直接完整校验，不预热证明。这些可丢弃的证明不持久化，也不授予权限。

完整的 Session 可见行元数据目录与规范事实在原 SQLite 写连接上以 FULL 耐久性共同提交。历史窗口按目录定位，仅重建选中来源的正文；正文或文本缓存缺失不要求重放整个 Session 前缀。原发射序号和行 ordinal 保持不变。投影失败或全局消息身份冲突会被明确记录，不额外拒绝原本已接受的执行事实；请求边界以内存在失败时，通过索引返回该 Session 历史不可用。存储错误仍使规范事务回滚。

对于逐项接受输出的 main 模型请求，完整文本段、来源、工具调用和 provider 工具结果在对应 observation 被接受的序号进入历史。读取更晚的边界或重新打开存储，不得改变较早边界的历史。首次目录安装在公开 Ready 前按来源顺序阻塞重建保留的规范事实与冻结成员，保留原始规范字节并明确记录历史坏来源。之后正常打开直接使用已安装目录，不重放历史；目录元数据丢失需要显式维护，不等同于普通选中正文缓存缺失。

首次升级使用既有获准长驻角色先维护启动，不依赖普通 launcher 的 30 秒启动观察窗口。未安装的 Root，或使用精确获准 executable 的 Supervised 部署，可运行 `host serve --root …`；它启动真实服务及正常恢复，不是只处理元数据的命令。OnDemand 部署必须使用匹配的获准 executable，以既有直接 `host candidate --root … --expected-root-id … --startup-attempt-id …` 角色启动，不使用 launcher 所有的 stdin。通过正常生命周期协调释放旧服务，保留部署模式、executable 准入和 Root 租约；观察持久化初始化完成及正常继续服务或真实关闭后，再使用普通 launcher。初始化完成前不发布 Ready，中断依靠事务恢复；不增加 timeout/progress 绕过机制或前台全前缀 fallback。

完整产物读取使用 `with_artifact`，选择 Session 或执行轮次范围。元数据与局部只读 BLOB reader 绑定到原写连接的同一快照；同步回调拥有目标资源，读取句柄关闭后才释放快照，避免每块重新打开并读取整个 payload。其他写入任务等待这次产物复制结束；即使等待方消失，已受理工作仍由原 owner 持有。调用方继续负责资源预算、摘要核验和取消策略。`read_artifact_chunk` 用于独立窗口，多次窗口调用不会保留顺序读取游标。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
