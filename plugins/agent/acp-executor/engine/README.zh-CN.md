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

# maka-external-agent

[English](README.md)

实现出站 ACP Agent 会话、安装和配置，将 Maka 执行端口与外部 Agent 进程相连接。原生包装层提供显式绑定的进程、文件和交互服务。这里负责调用外部执行器；面向编辑器的 ACP 服务端位于 `products/maka/acp`。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)

ACP 提示、附件、文件读取及输出载体通过原有进程与回调所有者保留完整支持内容。无损文本和写入分块保持不变，提示工作不另设总耗时期限。连接、配置观察与清理期限保留其生命周期含义。安装校验保留版本所规定的精确文件名、长度、摘要及分阶段发布、取消机制；公开安装 API 不代表已证明存在生产 UI 调用。
