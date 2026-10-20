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

使用 NSH 构建的实验性键盘优先 Agent 工作台，连接已有 Maka Host Root。

## 启动

如果 Host 尚未运行，在另一个终端启动：

```sh
cargo run -p maka-cli --bin maka -- host serve --root /path/to/root
```

随后在工作区根目录启动界面：

```sh
cargo run -p maka-terminal-shell -- --root /path/to/root
```

省略 `--session` 时，首页显示最近会话和 **New conversation**。添加 `--session SESSION_ID` 可直接打开已有会话。Host 必须提供 `agent`、`settings` 和 `receipts`；终端界面不会自动启动 Host。

在首页新建会话会打开 **Workspaces**，可选择 Host 发布的目录。在已有会话中新建时，复用该会话已获授权的工作区。创建完成后打开 **Models**，先选择已配置的连接和模型，再发送消息。如果没有可用连接，请在现有客户端中配置。

## 快捷键

| 按键 | 操作 |
| --- | --- |
| Ctrl+P | 打开命令面板 **Commands** |
| Ctrl+O | 打开会话列表 **Sessions** |
| Ctrl+G | 为当前会话打开模型列表 **Models** |
| Ctrl+N | 新建会话 |
| Ctrl+L | 跳到最新消息 |
| Alt+↑ | 聚焦会话，滚动整个对话 |
| Alt+↓ | 展开并聚焦最新一轮的活动面板 |
| Enter | 在输入框中发送，或激活选中的面板条目 |
| Shift+Enter | 在输入框中换行 |
| Esc | 关闭当前浮层；没有浮层时，请求停止当前运行 |
| Ctrl+C | 通过终端的 OSC 52 支持复制选中文本 |
| Ctrl+Q | 退出 |

模型选择使用 **Ctrl+G**，因为传统终端将 Ctrl+M 与 Enter 编码为同一个回车字符。粘贴请使用终端自身的粘贴命令。

在选择面板中输入关键词搜索，使用 ↑/↓ 选择，Enter 激活。Sessions 按标题、工作区或 ID 搜索会话；Models 搜索 Host 的模型目录。滚动或向下选择时会自动加载更多结果。Escape 关闭面板，不作选择。

## 对话与交互请求

向上滚动时自动加载更早消息并保留阅读位置。每轮有独立的原生活动面板，高度受当前视口约束，可单独滚动。接近底部时跟随新增活动；向上滚动后保留阅读位置。已完成的轮次默认折叠，最终回答显示在面板外。激活轮次标题可展开或折叠。完成状态和最终回答的位置依据 Host 记录的轮次元数据，单次模型响应结束不会提前关闭仍在继续的轮次。重开轮次后，思考正文保留展开状态。工具详情按需读取；重开包含已展开工具详情的面板时，会从顶部重新读取。

**Todo** 显示 Agent 的真实清单，包括当前事项和完成数量。Token 速度显示**上一次请求**的数据；上下文数据标明估算或最后已知值。没有可用数据时不显示相应指标。

打开待处理请求可查看其原始详情：

- 工具审批支持针对该次调用 **Allow** 或 **Deny**；允许前需读完全部原始参数。
- 附加权限支持 **Allow** 或 **Deny**，可选择请求提供的单次调用、当前轮次或当前会话范围。
- 问题支持已有选项、**Other answer** 和显式 **Skip**。
- 表单支持显式 **Decline** 或 **Cancel request**。完整表单填写和能力请求请在现有客户端中处理，终端尚不提供这些编辑器。

使用 Tab 切换请求控件、Enter 激活。审批、权限和问题可用 Ctrl+Enter 允许或提交答案；Ctrl+R 刷新已记录状态。Escape 只关闭请求视图，未回答的请求仍保持待处理。其他客户端先作答时，以显示的已记录结果为准。

## 保存状态与运行边界

草稿和操作回执保存在 `<root>/clients/terminal.sqlite`。每个会话保留自己的草稿，恢复使用原始 Host 与操作。切换会话前先保存当前草稿。恢复从最近一次确认保存的 revision 开始，之后未保存的输入可能在关闭时丢失。发送使用该已保存 revision；结果不明确时，请使用原操作恢复入口，避免重复提交。

可执行文件验证所选 Root 和在线 Host epoch。Ctrl+Q，以及 Unix 上的 SIGTERM，可独立于 JavaScript 和终端输出请求退出。退出时尝试恢复终端并关闭所拥有的 Host 请求和任务；清理未完成会返回错误。
