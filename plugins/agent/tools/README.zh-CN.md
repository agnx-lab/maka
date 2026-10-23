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

# maka-tools

[English](README.md)

分发带日志记录的工具调用，并提供 Code Mode。`RequestTools`、`RunTools` 和 `StepTools` 在各自执行作用域内保留工具选择。Code Mode 默认使用共用 JavaScript 运行时，也可通过模型设置 `codeModeLanguage: "python"` 选择随包提供的 Monty Python worker。两种语言沿相同的授权工具路径转发，共享会话 JSON 存储。工具结果、取消和实际清理是不同的结果。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
