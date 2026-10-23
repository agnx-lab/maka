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

# MCP 工具

Maka 使用官方 Rust MCP SDK（`rmcp`）接入 stdio 与 Streamable HTTP 工具服务器。原生 Agent 在激活时发现工具，再通过同一工具目录、审批流程和持久化日志提供直接调用、工具发现及两种 Code Mode 语言的访问。

## 配置

在产品 `installation.json` 的 `agent.mcpServers` 中配置服务器，再按[安装说明](../../../products/maka/bundle/README.zh-CN.md)生成并激活。已有安装可在[插件组合编辑器](../../../products/maka/cli/README.zh-CN.md)中修改 Agent 实例的 `configuration.mcpServers`，保存后重新激活 Agent。目前没有专门的 MCP 连接页面，默认列表为空。

以下片段合并进已有 Agent 配置；路径和服务器地址需要替换：

```json
{
  "mcpServers": [
    {
      "name": "local_docs",
      "transport": {
        "type": "stdio",
        "command": "/absolute/path/to/mcp-server",
        "args": ["--stdio"],
        "cwd": "/absolute/path/to/data",
        "passEnv": ["DOCS_API_KEY"]
      },
      "tools": ["search", "read_document"],
      "readOnlyTools": ["search", "read_document"],
      "preapproved": false,
      "startupTimeoutMs": 10000,
      "callTimeoutMs": 120000
    },
    {
      "name": "remote",
      "transport": {
        "type": "streamable_http",
        "url": "https://mcp.example.com/mcp",
        "headerEnv": {"authorization": "MCP_AUTHORIZATION"}
      },
      "tools": ["search"]
    }
  ]
}
```

Host 环境变量 `MCP_AUTHORIZATION` 应包含完整请求头值，例如 `Bearer …`。`headers` 设置字面量请求头，`headerEnv` 从 Host 环境读取值。不要把凭据放进 URL。HTTP 使用 Host 的代理设置且不跟随重定向；当前不提供 OAuth 登录或自动令牌刷新。

stdio 直接执行命令和参数数组，不经过 shell；`cwd` 必须为绝对路径。默认只继承 `PATH`、`HOME`、`USERPROFILE`、`SystemRoot`、`WINDIR`、`TEMP`、`TMP`、`TMPDIR`、`LANG`、`LC_ALL`。其他变量通过 `env` 或 `passEnv` 显式提供。服务器程序需要自行安装，Maka 不负责下载。仅使用 Node 的服务器与测试夹具需要 Node。

## 工具名、信任与结果

服务器名为 1–32 个 ASCII 字母、数字或下划线。公开工具名为 `mcp__<server>__<tool>`；远端工具名的其他字符转换为下划线。转换后重名，或显式选择的工具不存在，都会使激活失败。JavaScript 可调用 `tools.mcp__local_docs__search({...})`，Python 可调用 `await tools.mcp__local_docs__search({...})`。

省略 `tools` 表示允许该服务器已发现的全部工具；空列表表示不公开工具。`readOnlyTools` 显式信任指定远端工具可用于只读会话与计划。服务器的 `readOnlyHint` 无权授予此权限。`preapproved` 默认 false，每次调用需要确认；显式预批准仍须通过会话工具与工作区权限检查，等待批准后也会重查实时权限。计划模式仍要求显式只读信任。

安装 MCP 服务器意味着信任它。stdio 服务器以 Host 用户身份运行，不受会话文件系统沙箱限制，也可能在工具调用之外访问本机。HTTP 服务器自行控制文件系统、网络与外部效果。Maka 权限限制允许发起的调用，不能沙箱化服务器。启动和工具发现发生在 Agent 激活时，早于逐次审批。仅安装可信服务器，并仅转发需要的凭据。

日志与 Code Mode 保留完整 MCP `CallToolResult`，包含 `content`、`structuredContent`、`isError`、annotations 和 metadata；文本、图片和音频同时进入既有模型输出投影。`isError: true` 保留为服务器报告的工具错误。资源链接仅保留引用，不自动下载。服务器描述、schema 和返回文本均是不可信内容。

## 生命周期与限制

工具 schema 在本次激活内固定。收到工具列表变更通知后，目录停止接受新调用；需重新激活 Agent 获取新目录。每个新协议会话使用新绑定身份，之前排队的工作不能静默转到新服务器；重新激活后请提交新消息。配置的服务器启动失败会使 Agent 激活失败，并显示服务器名，不会静默略过。

每次调用仅发送一次，不自动重放，也不自动恢复过期协议会话。超时或取消会发送取消通知，并退役未结算的连接。stdio 清理等待本地进程组终止；缺少终态结果仍表示外部效果未知。HTTP 断连无法证明远端工作已停止，因此清理结果保持未确认。对于效果未知的操作，先检查外部系统，再决定是否重试。关闭 Agent 会关闭所有持有的连接。

最多配置 64 个服务器；每个服务器最多读取 64 页、1,024 个工具，序列化目录上限 1 MiB，随后仍须通过共享目录校验。stdio 消息、HTTP JSON/错误响应体及 SSE 事件在解析前限制为 16 MiB；HTTP POST SSE 还计算事件分隔符之间的注释。默认启动超时 10 秒、调用超时 120 秒；可配置范围分别为 1–300,000 ms、1–3,600,000 ms。

当前提供工具接入，不提供 resources/prompts 浏览器、roots、sampling、elicitation、tasks、OAuth 管理或旧版 HTTP+SSE 传输。

## 验证

```sh
cargo test -p maka-mcp
cargo test -p maka-agent-native mcp_native_direct_and_code_mode
```

测试使用真实子进程和 loopback HTTP/SSE 服务器，覆盖分页发现、结果与媒体、环境隔离、响应未结束时的大小限制、取消与进程清理，以及直接调用、Code Mode、审批和实时权限。
