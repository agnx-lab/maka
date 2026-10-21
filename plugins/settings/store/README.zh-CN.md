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

# maka-config

[English](README.md)

持久化 Root 的连接目录、凭据、模型目录和偏好设置。`ConfigurationStore` 使用已有的 Root 所有权。连接观察与凭据修订相互独立，客户端可以查看配置而不接收秘密值。这些记录控制执行，不属于模型会话历史。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)

连接、模型和凭据记录完整保留受支持的值，不设置应用层字节数或条数配额。Host 负责实际资源准入和操作结算。查询通过游标返回连续切片；完整首项可以超过分页打包目标。修订号、精确身份、规范结构和持久 OAuth 完成凭据仍是权威依据。
