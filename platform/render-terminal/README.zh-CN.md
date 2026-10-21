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

# maka-render-terminal

[English](README.md)

使用 Ratatui 和 Crossterm 在终端呈现经过校验的 UI 组合。`application` 驱动输入、已提交帧、原生编辑器和关闭流程，`markdown` 负责文本渲染。组合与服务由产品提供；渲染器不选择 Agent 页面，也不内嵌 JavaScript 执行。

前台按输入、工作完成和实际可见的截止时间唤醒。输入、recipe 变化与原生观察请求新帧；可见动画和耗时标签安排下一帧。不变的画面不重复布局、缓冲区差分或终端输出。

各原生所有者为不可变场景 recipe 中的纯节点保留测量结果和堆叠布局的相对几何。查询按需保留，范围限于当前及前一次实际测量过程的工作集；未使用的约束随过程退休，不累积历史窗口宽度。替换 recipe 会释放其缓存；组件和自适应面板仍动态测量。派生几何不占原生输入和发布容量，缓存未命中时使用普通布局。原生编辑器保留正文和滚动条探宽使用的两份文本布局。这些缓存不发布输入几何，也不绕过原有写入确认检查。

多行编辑器接收完整、可表示的文本，即使最新编辑超过旧历史的保留目标，也保留这一步撤销。剪贴板文本按实际返回的分配大小通过共享原生预算准入；平台剪贴板读取器在内部完成该分配。捕获的快照保留原始字节，不受后续原生编辑影响。

唯一且有界的输出所有者写入完整 ANSI 缓冲区，不阻塞前台输入。编码时 Ratatui 的差分基线已经推进；该缓冲区全部写完并刷新之前，前台不会生成下一次差分。只有写入完成才提升对应的命中几何，这不等于终端已经显示的确认。部分写入的缓冲区不能丢弃。若期间已有更新的业务帧提交，旧写完帧只提供实际屏幕几何，不能恢复已退役回调，也不能清除新帧的焦点、打开确认或重绘需求。陈旧业务输入按原身份拒绝。

必要且可失败的原生准备先于逻辑原子发布。挂载固定一种原生 recipe 实现类型，切换实现通过新子键或重新挂载。纯文本解析作为有界后台准备执行，拒绝或过时的结果保留已提交文档。原生观察复用有界发布生命周期；入队不代表原命令回执已确认。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
