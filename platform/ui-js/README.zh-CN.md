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

# maka-ui-js

[English](README.md)

为 `maka-ui-runtime` 执行 JavaScript 组件回调。`execution` 配置一个有界接收任务、空闲时休眠的后台 worker。激活、挂载、更新、输入、捕获与关闭都通过异步 job 执行。Reducer 保持有界同步计算，从原已提交状态提出候选，不等待 I/O。

Rust UI 所有者校验并提交结果批次。迟到结果不能恢复已退役挂载。原生输入资源、精确操作回执及已接受执行的真实清理仍归原所有者管理；迁移执行线程不代表这些工作已经完成。

调用任务持有共享的不可变状态和 props，避免在 UI 所有者上复制整棵 JSON 树。参数校验和编码在后台直接读取这些快照。JavaScript 仍接收独立的 JSON 值，脚本修改参数不会改变 Rust 保留的快照。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
