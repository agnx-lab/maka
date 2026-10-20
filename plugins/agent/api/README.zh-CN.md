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

# maka-agent-api

[English](README.md)

包含 Agent、Session、Executor、WorkHub 的协议声明以及 Agent 配置 schema。原生服务与 SDK 生成共用这些定义。调用方只需要公开契约时，可以依赖这里，无需引入 Agent 执行引擎。

独立 Desktop 通过同一个 Root 绑定的[公共 Host client](../../../platform/host-runtime/src/application/host/client.rs)使用持久 Agent 服务，不直接打开 Agent 存储，也不嵌入执行引擎。字段及 schema 以 [Agent 契约](src/contract.rs)和[交互契约](src/contract_interactions.rs)为准。

`history` 分页保持同一个 `through` 边界，`watch` 从最后的 `nextAfter` 继续，并发布队列与待处理交互更新。`send` 接受工作并返回其身份，`stop` 指定该次 invocation。重连后通过 `pendingInteractions`、`interaction` 和 `respond` 重建并回答原请求。修改保留原操作回执；丢失响应不代表可以重新发送。参见[原生接入验收](../native/README.zh-CN.md)及[传输所有权与退出方式](../../../platform/transport/README.zh-CN.md)。

历史行引用使用 `position: { sourceSequence, itemIndex }`，先按产生该行的原始事件排序，再按该事件内的行序号排序。游标、来源读取、提示词锚点及工具图片引用共用这一位置。`through` 和 `{ kind: "event", sequence }` 仍指向原始事件；模型完成时产生的所有历史行都归属于该完成事件。

`search` 在所选 Session 的历史中按字面量查询，显式传入 `caseSensitive` 和 `wholeWord`。忽略大小写使用 Unicode 简单大小写折叠，不做规范化；全字匹配要求两侧不紧邻 Unicode 单词字符（字母、组合标记、十进制数字、连接标点或连接控制符）。以 `after: null` 开始，分页时保持查询、选项和 `through` 不变，沿 `nextAfter` 继续，即使当前页没有命中；null 表示扫描结束。每条命中标明原始字段，可通过 `source` 精确读取。搜索命中以 `position` 和原始 `field` 定位。用相同 `through` 调用 `history`，位置为 `{ kind: "row", position: hit.position, field: hit.field === "thinking" ? "thinking" : "body" }`，可打开空工具行及明确指定的内部工具；后续隐藏规则不变。已有字节游标仍用于续页。

Executor 的 `execute` 通过名为 `attachments` 的单个可选二进制资源接收不可变文件。资源 metadata 是按顺序排列的 `{ name, mimeType, bytes, digest }` 数组；资源内容按同一顺序连接各文件的原始字节，偏移由前项长度累加得到。必须通过实际请求资源读取，Host 字节预算和清理责任仍归原请求所有；metadata 本身不授予访问权限。

`fork` 根据来源版本，从完整一轮或全部当前历史创建旁支会话。所包含的工作必须已结算；后续轮次仍在运行时，也可分叉更早的完整轮次。新 Session 保留这段历史和附件，共享已验证的工作区，收窄权限且不带入运行中的工作；`branch` 标明来源。丢失响应后使用原操作引用恢复；Host 结果仍未知时，`forkReceipt` 可查找已发布的分叉。返回 null 不代表操作未发生，也不授权创建替代分叉。字段见[分叉声明](src/contract_fork.rs)。

`diagnostics` 在固定的 `through` 边界内，从新到旧分页读取执行、模型步骤及自动标题失败。`watchDiagnostics` 在订阅时及变化后发送当前诊断版本；重连后重新读取最新页。这些会话范围内的运行记录保留原 invocation/event 标识和安全的类型化原因，不包含提供方响应正文或复制来源会话的失败。诊断版本独立于对话历史，因此回答结束后才发生的标题失败仍可发现。

`context` 返回所选 Session 最近一次已准备的 Main 请求，包括运行中或已中断的请求。它按消息角色和工具定义展示原始 Host 输入构成；token 数为估算，提供方报告的实际输入用量单独返回。固定 `through` 可在重连后读取同一结果。不存在的请求或缺失的测量保持 null，复制的历史不算新 Session 自己执行的请求。字段见[上下文声明](src/contract_context.rs)。

`gitStatus` 和 `gitDiff` 通过同一个 Agent 服务读取所选 Session 的仓库，也适用于 WorkHub 协调者会话。状态区分已暂存、工作区和未跟踪的变化；同一路径可以同时出现在已暂存和工作区部分。diff 分别比较 HEAD 与索引、索引与工作文件，或空文件与未跟踪文件。尚无提交的仓库返回 null `head`；没有仓库的工作区返回 null。路径以 `bytesBase64` 保留 Git 文件名的原始字节，重命名呈现为删除加新增。

这些查询提供当前的只读观察，不是仓库快照。状态响应不超过 512 KiB，结果不完整时明确标记 `truncated`。单文件 diff 读取完整输入，区分文本、二进制、未变化和不支持的情况；文本补丁完整返回。输入和补丁在内存中缓冲，仓库元数据、control 文件和条目数量检查仍然适用。所选工作区必须是仓库根目录；外部过滤器、稀疏检出、子模块等不支持的仓库特性会明确失败，不执行配置中的命令。工作区或比较基准变化后应重新查询。协议类型见 [Git 声明](src/contract_git.rs)；已完成工作的不可变补丁仍使用 `snapshotPatch` 和 `readPatch`，遵循各自的捕获限制。

`listFiles` 列出所选 Session 工作区中一个目录的直接子项，`watchFiles` 通知该目录需要重新读取；也适用于 WorkHub 协调者会话。`path` 为 null 表示工作区根目录，其他路径使用 `WorkspacePath`，以 `bytesBase64` 保留相对文件名的原始字节。符号链接作为叶子展示，不进入其目标；排除 `.git`、工作区标记及 Host 私有状态/control 目录，模型配置为无限制访问时也不例外。私有状态内仅允许读取 Session 已验证的受管 worktree。每页最多 128 项、512 KiB，最多扫描 100,000 个直接子项；路径最多 4,096 字节、64 层。平台无法表示的文件名明确报错，不做有损转换。

先订阅并等待 `ready`，再读取第一页，并核对它与 `ready` 的 `directoryIdentity` 一致；通过 `next` 继续分页。收到 `changed` 后丢弃游标，从第一页重新读取。分页反映实时目录，并发编辑可能改变分页结果。通知可合并，不提供历史事件重放；重连后重新订阅和读取。检测到所选子目录删除或替换后，以 `unavailable` 结束订阅；Session/工作区授权失效或原生监听失败则以错误结束。macOS 上，空闲工作区根目录的移动可能直到下一次通知、读取或重连才被发现。离开目录时关闭订阅。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
