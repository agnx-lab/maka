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

消息交付保留准备后的内容、可编辑原文和最初提交的摘要。内部记录保存这些不同事实，编码后可能大于公开请求；写入与精确消息读取不再对整条编码记录重复套用请求大小限制。队列总量预算和调用方传输预算分别管理。通过 `editable_message` 按 Session、Turn 和消息身份读取一条已拥有的来源。

来源消息、执行器工具调用与生成的工具结果共享 Session 的可见消息身份空间。无论哪一方先声明，准入都依据规范事实检查冲突，包括执行器完成或调用终止时为待完成工具补出的结果。只有实际结果身份被占用；仅后缀相同不会预留身份。

读取 checkpoint 时会校验每个原始来源承诺，包括被最新摘要隐藏的前驱。前缀摘要依次绑定作用域、有序的序号／字节长度／原始字节记录，最后绑定 high-water；相同选择范围的多个边界可共用一次流式扫描。继承链选择和归档请求边界仍分别校验。该内部编码不保留旧格式兼容路径。

存储打开期间，同一选择范围可复用已验证的 checkpoint 链，并延伸已认证的前缀。写连接的实时确认将证明绑定到读连接的固定快照或已取得写锁的事务；受保护历史的变更、外部提交、schema 变化及写入失败会撤销失效证明。选择范围、准入、结算和归档仍实时校验。仅含 opening 的读取直接完整校验，不预热证明。这些可丢弃的证明不持久化，也不授予权限。

历史索引准备先选择有序的规范事实身份，再在同一事务中逐个读取，不以证据总条数或总字节数拒绝重放。投影仍校验单项内容，历史分页仍遵循传输预算；派生行和进度共同提交。每个有界批次在原 SQLite worker 上执行，存储 owner 等待其结果；独立读连接可继续推进，其他写任务等待该批次结束。重放内存可随选定的 step 增长，已受理的准备工作由原存储 owner 执行至结束。

对于逐项接受输出的 main 模型请求，完整文本段、来源、工具调用和 provider 工具结果在对应 observation 被接受的序号进入历史。准备更晚的边界或重新打开存储，不得改变较早边界的历史。升级改变投影规则时会清除可重建的历史行、文本和进度，规范事实保持不变。

完整产物读取使用 `with_artifact`，选择 Session 或执行轮次范围。元数据与局部只读 BLOB reader 绑定到原写连接的同一快照；同步回调拥有目标资源，读取句柄关闭后才释放快照，避免每块重新打开并读取整个 payload。其他写入任务等待这次产物复制结束；即使等待方消失，已受理工作仍由原 owner 持有。调用方继续负责资源预算、摘要核验和取消策略。`read_artifact_chunk` 用于独立窗口，多次窗口调用不会保留顺序读取游标。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
