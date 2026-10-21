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

# maka-jev

[English](README.md)

实现结构化 Jev 评估和端点设置。`decision` 校验问题与响应，`settings` 管理配置及绑定到端点的凭据。调用使用作用域内的 HTTP 资源。返回的概率是提供方给出的答案，超时可能意味着远端结果仍未知。

评估、完整响应和绑定端点的凭据不再附加本地数量或字节上限。调用方配置的正超时仍是明确的执行期限；取消、网络失败和无法确认的清理保留远端结果的不确定性。问题和回答保留原始 ID、选项及评分结构、概率校验和 JavaScript 可精确表示的用量计数。引擎缓冲完整 JSON，不承诺内存有界或供应商接受每个本地有效的评估。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
