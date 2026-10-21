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

# maka-session-import

[English](README.md)

发现并转换 Claude、Codex 和 OpenCode 的外部会话格式。`Transcript` 是统一的导入表示，`source` 与 `catalog` 负责发现。原生 Agent 服务验证所选来源，在同一事务中写入完整记录并发布新 Session；重试核对原始规范记录。适配器将重复使用的外部工具 ID 归一化为不同的调用实例键；新导入的 Session 拒绝重复的 ToolCall 键。导入不执行文本中的指令，也不替代 Host 的规范事件写入方。

来源配置保留完整名称和路径。目录与预览分页保留原始选择和续读位置；预览不替代导入的完整记录。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
