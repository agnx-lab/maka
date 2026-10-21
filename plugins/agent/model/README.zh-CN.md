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

# maka-model

[English](README.md)

通过提供商适配器构造模型请求并接收流式响应。`ModelExecutor` 管理请求，`Conversation` 构造会话输入，`reasoning` 处理推理配置。凭据通过 `AuthResolver` 提供，调用方传入已授权的网络与计量上下文。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)

HTTP、SSE、WebSocket 及持久重建保留完整的规范响应条目和文本。分块与有界通道施加背压，不另设整个响应的配额。可丢弃的续接缓存允许淘汰，恢复依据仍是规范历史。请求 token 选择、提供商协议校验及原有空闲、取消清理与内容保留分开处理。
