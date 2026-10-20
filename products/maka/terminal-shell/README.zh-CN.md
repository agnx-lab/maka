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

# Maka NSH 终端界面

通过公共 Maka Host 服务工作的实验性第一方 NSH 应用。原生可执行文件选择一个已有 Root 和 Agent 会话，JavaScript 获得已连接的客户端。Host 必须提供 `agent`、`settings` 和 `receipts` 端点。

如果所选 Root 的 Host 尚未运行，在另一个终端使用现有服务入口启动：

```sh
cargo run -p maka-cli --bin maka -- host serve --root /path/to/root
```

随后在工作区根目录运行独立界面，明确选择一个已有会话：

```sh
cargo run -p maka-terminal-shell -- --root /path/to/root --session SESSION_ID
```

可执行文件验证绑定 Root 的发现记录和在线 Host epoch。持久客户端日志位于 `<root>/clients/terminal.sqlite`；草稿和操作回执按原始 Host 与提供者身份归属。现有产品和发布启动器保持不变。

Ctrl+Q 可独立于 JavaScript 和终端输出请求退出。Unix 上的 SIGTERM 具有相同作用。界面尝试恢复终端后，关闭所拥有的 Host 请求和原生任务；恢复或清理未完成会返回错误。

草稿恢复从最近一次确认保存的 revision 开始；之后尚未确认保存的输入可能在关闭时丢失。发送读取准确的已保存 revision；结果不明确的操作保留用于回执恢复，避免重复发送。
