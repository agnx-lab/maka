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

# maka-agent-engine

[English](README.md)

协调 Agent 运行、模型步骤、工具、移交和恢复。`Engine` 使用显式业务端口与已授权资源，`RunInput` 和 `RunWork` 描述获准工作。模型历史投影与 UI 呈现相互独立。原生 Agent 服务管理激活并委托这里执行；平台 Host 和 UI crate 不依赖本引擎。

续跑保留原运行的输入和已完成历史。新的模型步骤使用当前已授权的工具状态。

视觉模型的历史投影按既有 artifact 存储上限读取每张已授权图片的完整内容。复制会话通过目标会话持有的引用解析保留的图片。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
