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

# maka-ui-runtime

[English](README.md)

管理组件激活、状态、输入分发和帧的原子更新。`Application` 执行经过校验的 UI 组合，`NativeRegistry` 提供编译进程序的工厂和场景编译器。需要 JavaScript 时，由产品通过 `backend` 模块注入执行适配器。

回调提供状态、帧和一次性动作的候选结果。Rust 唯一持有已提交状态，每次推进一个逻辑事务。后台准备不发布动作，也不替换原生资源；原渲染所有者检查挂载与帧版本依据，完成必要且可失败的原生准备后，原子提交整个批次。布局与绘制消费已提交帧，终端渲染器保留自己的布局与场景语言。

已提交帧与已接受调用共享不可变的 JSON 状态和 props 快照。每个回调结果创建独立候选状态，拒绝候选会保留原快照。原生回调借用这些值；JavaScript 适配器则持有它们，直到已接受的后台任务完成。插件 JSON 协议保持不变。

原生默认动作与编辑不等待 JavaScript。组件在已提交 props 中声明支持的原生行为，业务输入按顺序入队。原生观察具有有界发布托管：入队不等于发布，只有原逻辑提交才能确认观察。拒绝候选保留精确原值，在业务或来源实际变化后重试。只有完整且可替代的快照允许合并；命令回执和终局结果保持顺序。

原生编辑器保留缓冲区并提供有界捕获。同一挂载固定一种原生 recipe 实现类型；切换实现需要新子键或重新挂载。类型不匹配会在改变已提交原生所有者之前拒绝候选。异步变更沿用原始回执和调用方身份。操作反馈携带已确认的调用方提交信息（若存在），让组件直接记录精确历史位置；该位置本身不证明业务结果或原生编辑已完成。替换组件必须等待已接受工作和原生资源完成清理，丢弃帧或结果观察者并不足够。相关实现见 `actions`、`operations`、`editor`、`checkpoint` 和 `publication`。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
