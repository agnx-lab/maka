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

# maka-python-runtime

[English](README.md)

可选的 Python Code Mode 后端，使用固定版本 Monty 1.3.0 Rust crate，不依赖系统 Python、pip 包或原生扩展。在连接的模型设置中启用 Code Mode 并选择 Python（`codeModeLanguage: "python"`）；默认语言仍为 JavaScript。

每个 cell 启动独立的 `maka-python-worker`，默认位于 Host 可执行文件旁。使用 `cargo build -p maka-python-runtime` 构建；开发运行脚本和原生发行包自动包含它。嵌入方可通过 `CodeExecutor::with_worker` 提供明确路径。

支持顶层 await，以及 `await tools.name(input)` 或关键字参数形式的 JSON 工具调用。使用 `asyncio.gather` 并发调用；若需保留部分成功结果，在各协程内部捕获异常。Monty 不提供完整 CPython 标准库，也不支持 `asyncio.create_task`、`TaskGroup` 或 `gather(return_exceptions=True)`。文件、网络和进程操作必须通过已授权 Host 工具。

共用 cell 运行时管理工具效果、输出和会话 JSON 存储。取消或丢弃观察者后，仍须等待实际清理才完成。Python 全局变量不跨 cell 保留；`store`/`load` 值可跨 cell 和语言共享。worker 边界只接受无损 JSON，嵌套上限为 128 层。

默认限制为解释器分配 64 MiB、累计执行 30 秒；进程分配器为协议和解释器开销保留有界余量。启动和等待 Host 不计入执行时间，编译计入。取消会终止并回收 worker，再等待已接收的工具效果清理。全局分配器只安装于 worker 二进制，不影响 Host。

[源码](src/lib.rs) · [Cell 指令](../../plugins/agent/tools/src/cell/python.md)
