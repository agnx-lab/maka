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

# maka-agent-facts

[English](README.md)

定义持久化与执行共用的 Agent、Session 和 Run 事实，包括事件、消息、模型选择、工具调用、计量和续接记录。这里只包含数据类型，不依赖执行引擎或 SQLite。执行策略应留在所属服务中，而非加入持久化事实。

非法工具参数会被保留并交回模型修正，不会执行工具。取消和恢复后仍保留相同证据。

此前工具参数格式的历史及 bundle 无法重放。使用 `maka host init --root NEW_EMPTY_DIRECTORY` 初始化独立空 Root；已有数据不会自动迁移或删除。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)

成功的工具结果保留完整原始证据和有序冻结投影。精确长度、摘要、已确认操作回执及 Session 媒体所有权仍为必要条件。原始类型化 JSON 必须能通过依赖所支持的解码表示往返；归档规范编码以迭代方式遍历 JSON，保留既有键与数字编码。模型 token 预算单独选择请求视图，不改变所保留的证据。
