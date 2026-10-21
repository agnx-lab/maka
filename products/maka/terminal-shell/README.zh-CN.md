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

使用 NSH 构建的实验性 Agent 工作台，支持鼠标与键盘操作，连接或启动 Maka Host Root。

## 启动

在仓库根目录构建产品包及同目录的原生启动器，再启动界面：

```sh
npm run build --workspace @maka-agent/product
cargo build -p maka-cli --bin maka
cargo run -p maka-terminal-shell -- --root /path/to/root
```

终端先发现并验证指定 Root 的在线 Host；没有在线 Host 时，使用同目录的 `maka` 原生启动器按既有部署配置启动。无需另开终端运行 `host serve`。启动器不会从 `PATH` 查找。`--root` 指定业务 Host Root；默认产品的业务 Root 通常为 `~/.maka/application/host`。

省略 `--session` 时，居中的输入区域会显示 **Choose workspace**，选择工作区后即可开始对话。添加 `--session SESSION_ID` 可直接打开已有会话。Host 必须提供 `agent`、`settings` 和 `receipts`。退出终端只关闭自己的连接，Host 按既有生命周期和空闲策略退出。

点击 **Choose workspace** 或按 Ctrl+N 打开 **Workspaces**，选择 Host 发布的目录。在已有会话中新建时，复用该会话已获授权的工作区。创建完成后打开 **Models**，先选择已配置的连接和模型，再发送消息。如果没有可用连接，请在现有客户端中配置。

**Sessions** 在宽终端中显示为可收起的侧栏，窗口较小时改为对话框；点击顶部的浏览按钮或按 Ctrl+O 打开。拖动分隔线，或使用 **Narrower** / **Wider** 调整侧栏宽度。可浏览全部工作区、当前会话的精确工作区或项目。项目旁的 **+** 在其获授权目录中新建会话；**New in current workspace** 复用当前会话的工作区。独立 worktree 不一定属于父项目。

## 快捷键

| 按键 | 操作 |
| --- | --- |
| Ctrl+P | 打开命令面板 **Commands** |
| Ctrl+O | 打开会话列表 **Sessions** |
| Ctrl+G | 打开 **Models**，选择模型和推理档位 |
| Ctrl+N | 新建会话 |
| Ctrl+L | 跳到最新消息 |
| Alt+↑ | 聚焦会话，滚动整个对话 |
| Alt+↓ | 展开并聚焦最新的过程组，或相应的平铺内容 |
| Tab | 切换控件 |
| Enter | 在消息输入框中发送、选择搜索结果，或激活已聚焦的控件 |
| Shift+Enter | 在输入框中换行 |
| Esc | 在 **Models** 内先返回上一级，再关闭；其他位置关闭浮层或已聚焦的侧栏，聚焦对话时请求停止 |
| Ctrl+C | 通过终端的 OSC 52 支持复制选中文本 |
| Ctrl+Q | 退出 |

模型选择使用 **Ctrl+G**，因为传统终端将 Ctrl+M 与 Enter 编码为同一个回车字符。粘贴请使用终端自身的粘贴命令。

在搜索框中输入关键词，使用 ↑/↓ 选择，Enter 激活。Sessions 在所选范围内按标题进行区分大小写的字面子串搜索，最多 128 个 Unicode 字符；Host 在分页前应用搜索条件，不搜索消息正文、路径或 ID。模型搜索使用 Host 的模型目录。滚动或向下选择时会自动加载更多结果。

点击输入区域的模型名称或按 Ctrl+G 打开 **Models**，再选择 **Model** 或 **Effort**。推理档位来自所选模型公布的策略，仅在支持时显示 **Off**。**Default · Use connection default** 清除显式档位，跟随连接的默认策略；重新选择同一连接下的同一个模型会保留显式档位。在模型或档位列表中按 Escape 或点击 **Back** 返回菜单，再按 Escape 或点击 **Close** 关闭，焦点返回原控件，草稿和光标位置保持不变。

## 对话与交互请求

向上滚动时自动加载更早消息并保留阅读位置。每轮有独立的原生活动面板，高度受当前视口约束，可单独滚动。接近底部时跟随新增活动；向上滚动后保留阅读位置。已完成的轮次默认折叠，最终回答显示在面板外。激活轮次标题可展开或折叠。完成状态和最终回答的位置依据 Host 记录的轮次元数据，单次模型响应结束不会提前关闭仍在继续的轮次。重开轮次后，思考正文保留展开状态。工具详情按需读取；重开包含已展开工具详情的面板时，会从顶部重新读取。

**Todo** 位于输入区域上方，显示 Agent 的真实清单，包括当前事项和完成数量。Token 速度显示**上一次请求**的数据；上下文数据标明估算或最后已知值。没有可用数据时不显示相应指标。

展开包含图片的工具结果可查看缩略图，激活 **Open image** 可打开较大的预览。Escape 或 **Close** 返回对话并恢复焦点，不会停止当前运行。两处都使用 Host 保留的原始图片，即使工作区文件后来改变也不受影响。启动时会自动检测 Sixel 支持和字符单元的像素大小；其他情况下，图片自动使用彩色 Unicode 半块和象限块渲染。调整终端窗口大小后，图片会持续使用 Unicode，直到重新启动此 shell。不支持的图片格式会在结果中显示错误。

打开待处理请求可查看其原始详情：

- 工具审批支持针对该次调用 **Allow** 或 **Deny**；允许前需读完全部原始参数。
- 附加权限支持 **Allow** 或 **Deny**，可选择请求提供的单次调用、当前轮次或当前会话范围。
- 问题支持已有选项、**Other answer** 和显式 **Skip**。
- 表单支持显式 **Decline** 或 **Cancel request**。完整表单填写和能力请求请在现有客户端中处理，终端尚不提供这些编辑器。

使用 Tab 切换请求控件、Enter 激活。审批、权限和问题可用 Ctrl+Enter 允许或提交答案；Ctrl+R 刷新已记录状态。Escape 只关闭请求视图，未回答的请求仍保持待处理。其他客户端先作答时，以显示的已记录结果为准。

## 保存状态与运行边界

草稿和操作回执保存在 `<root>/clients/terminal.sqlite`。每个会话保留自己的草稿，恢复使用原始 Host 与操作。切换会话前先保存当前草稿。恢复从最近一次确认保存的 revision 开始，之后未保存的输入可能在关闭时丢失。发送使用该已保存 revision；结果不明确时，请使用原操作恢复入口，避免重复提交。

可执行文件验证所选 Root 和在线 Host epoch。Ctrl+Q，以及 Unix 上的 SIGTERM，可独立于 JavaScript 和终端输出请求退出。退出时尝试恢复终端并关闭所拥有的 Host 请求和任务；清理未完成会返回错误。
