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

# maka-host-runtime

[English](README.md)

执行经过校验的服务组合，负责调用准入、激活和退役。产品提供原生工厂，并按需注入 JavaScript 执行适配器。所有者与客户端 API 见 `application`；编写原生服务可从 `application/native_context.rs` 开始。

提交的配置表示期望状态，实例完成收敛后才成为有效状态。更新保留未改变的激活及其绑定；替换必须等待已接受调用和原生资源完成清理。操作回执区分业务结果与清理状态，丢失响应后应查询原回执。

订阅的提供方或调用方退役时，Host 请求取消原订阅调用，并在原处理程序及其原生资源完成清理后关闭提供方。已受理的查询和变更仍保留原有完成与清理语义。订阅和变更进度通过队列背压和消费者需求控制交付，不另设单项大小额度。

Host 和 JavaScript 请求桥不对服务输入、输出、配置和原始回执另设 JSON 大小额度。已受理的输入仍占用共享容量，直至实际工作完成清理；这不限制输出的内存占用。schema 校验、JavaScript 数值精度和 IPC 帧要求在各自边界生效。JavaScript 无法精确表示的回执仍保存在原始日志中。

完整配置文档包含冻结的远端契约和包引用，准入、持久化与管理保留原始完整字节。读取时固定配置版本并逐页获取；较大文档通过不可变资源替换。Host 的并发输入与资源托管使用装填窗口：较大的调用独占整个窗口，直到实际结清，不再仅因完整大小而拒绝。

持久化由已持有的 EventLog 提供。插件数据归属于 Root、部署和实例；激活 epoch 标识一次执行，不会创建新的数据所有者。

内置 `maka.host.logs` provider 从同一个 Root 所有者派生日志源。其 `HostLogs` 导出（`maka.host.Logs`）提供有界原始字节 `read` 查询和可取消的 `watch` 订阅。组合必须绑定并显式授权这些方法；注册本身不授予访问权，请求也不能选择其他 Root 或文件。页和游标字段见生成的 SDK 契约。

Host runtime 为初始化、配置采用、操作持久化和实际清理失败记录安全分类及原始归属 ID，不复制任意插件异常、SQL 消息或请求内容。日志不改变就绪状态、操作回执或清理归属；不确定的变更仍须查询原回执。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
