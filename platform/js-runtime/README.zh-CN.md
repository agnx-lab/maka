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

# maka-js-runtime

[English](README.md)

提供插件 guest 和持久 JavaScript REPL 共用的 V8 执行机制。`plugin` 负责受限的 guest 执行，`repl` 与 `CodeExecutor` 支持跨调用保留变量的代码单元。嵌入方提供可调用能力并管理其资源；JavaScript Promise 完成不代表它启动的原生工作已完成清理。

嵌套工具调用通过嵌入方的原始执行器传递完整 JSON 参数和结果。代码单元可以消费较大的工具结果并发布简短摘要；完整的单元结果、输出和暂存值受配置的 V8 堆和执行期限约束。已受理的工具工作会被跟踪到清理完成，包括 JavaScript 不再观察其 Promise 的情况。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
