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

# maka-processes-native

[English](README.md)

提供工作区命令、终端和协议进程服务。`protocol` 处理结构化子进程通信，`terminal` 管理交互终端资源。配置给出安装权限上限，每次调用仍需自己的授权。关闭观察者不会结束对运行中进程的所有权。

活动终端屏幕和后台进程日志来自原 worker handle。单纯输出不会更新持久化资源的 revision、`updatedAt` 或 `watchList`；启动、状态、实际尺寸变化及最终输出仍持久化提交。详情观察允许同一持久化 revision 下的新输出。Host 崩溃后，孤儿恢复保留最后一次持久化快照，它可能早于最后的活动输出。

`health`（默认模型工具 `BackgroundTaskHealth`）读取当前会话原有的进程，包括仍保留的活动日志。`updatedAt` 表示最近一次持久化资源更新，不代表输出活动或端点健康。只有 `include_logs: true` 才返回日志。可选 HTTP(S) 地址由绑定的 Web 服务在独立网络授权下探测；进程状态与端点观测互相独立，端点响应不能证明监听进程归属或浏览器就绪。健康检查及嵌套探测均保留原始操作回执。

Terminal [原始输出契约](../../api/README.zh-CN.md)复用原有 PTY owner 及由该资源拥有的两个匿名临时文件，保留完整 replay。临时存储随输出增长，原资源和全部现有 reader 释放后回收。关闭输出观察不会改变进程或控制器，现有顺序控制和 mutation 回执仍保留原有权威。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
