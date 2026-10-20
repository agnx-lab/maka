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

嵌套工具调用通过嵌入方的原始执行器传递完整 JSON 参数和结果。代码单元可以消费较大的工具结果并发布简短摘要；单元结果、输出和存储值的预算在各自发布边界生效。已受理的工具工作会被跟踪到清理完成，包括 JavaScript 不再观察其 Promise 的情况。

可信 provider SDK isolate 使用 V8 默认堆策略，通过字节预留安排请求，并将预留持有至原生资源清理完成。堆上限回调会标记共享运行时失败；原生内存耗尽仍可能终止进程。生成代码与插件 guest 仍使用各自配置的限制。

模型 SDK 通过现有请求范围内的 HTTP 能力提交结构化 JSON，由 Rust 编码为自有字节，整个请求无需放进一个 JavaScript 字符串。此桥接仅接受 SDK 构造的 JSON 对象，不是任意 JavaScript 值或通用 Fetch 正文接口。完整请求仍会缓冲；编码期间检查取消，但同步值转换和单次写入不可抢占。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
