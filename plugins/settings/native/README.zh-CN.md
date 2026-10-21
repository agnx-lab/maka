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

# maka-settings-native

[English](README.md)

通过原生应用服务提供 Root 设置。`provide` 注册工厂，`contract` 定义接口，`network`、`preferences` 和 `usage` 实现设置操作。激活时接收已有 Root 所有者并使用设置存储。读取设置不会暴露凭据值。

语言是 Root 共享的单一偏好，使用现有策略 revision 更新。`assistantPreferences` 同时返回保存的选择和解析后的 `en`、`zh-CN` 或 `zh-TW`。未设置时遵循产品配置的 locale；显式 `auto` 遵循原生启动语言，显式语言则覆盖两者。激活配置接受 `zh-Hant-HK` 等 locale 标签，在使用前统一解析。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)

Settings 完整保留配置、标签、搜索输入和认证资源。分页目标约束每次连续切片，不限制完整目录；单个较大项目仍能完整返回并推进游标。Host 负责实际资源准入和结算，Settings 保留领域语法、CAS 和凭据权威。
