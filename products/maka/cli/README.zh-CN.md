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

# maka-cli

[English](README.md)

构建 `maka` 可执行文件，负责终端产品启动、Host 服务、ACP，以及应用检查、组合和安装命令。产品启动将原生工厂和 JavaScript 执行适配器接入平台所有者。

在仓库根目录执行 `just run` 或 `just run --help`。开发构建读取 `../bundle/dist`，`just run` 会先重建该 bundle 再启动 Cargo。发布构建将其嵌入程序。桌面端使用独立的同目录可执行文件。

[源码](src/main.rs) · [Cargo 配置](Cargo.toml)

运行 `maka terminal-shell` 打开 [NSH 终端工作台](../terminal-shell/README.zh-CN.md)。默认使用 `~/.maka/application/host`，支持 `--root DIRECTORY` 和 `--session SESSION_ID`。同目录的 `maka-terminal-shell` 负责 Host 发现、启动和终端清理，原生预览包会包含它。`maka terminal-shell --help` 可查看参数。

首次安装会在 TUI 内确认工作目录信任，然后才启动 Host、启用项目技能与工具。Tab 或方向键选择，Enter 确认，Esc 退出；默认选中退出。确认后在同一终端画面中初始化并进入产品。再次打开使用已保存的安装，不重复询问。

启动时自动发现并连接本机 Host；不存在则自动启动。TUI 与 Desktop 可以共享同一个 Host。默认退出 UI 时，若没有其他客户端连接，Host 也会退出。TUI 中可用 Ctrl+Alt+Q 仅关闭界面，让已接受的后台任务继续运行，重连后观察结果。

`host serve` 和 `application serve` 输出的 JSON `ready` 行包含 `rootId`、`hostEpoch` 和 `socketPath`。接入时使用该 epoch，并核对实际 Host 身份。Unix socket 位于系统选择的临时目录下的私有子目录中，可通过 `TMPDIR` 指定其他父目录。规范化后的 socket 路径须满足可移植的 100 字节上限；若启动报告路径过长，请选择更短的父目录。

无界面命令（`host`、`application serve`、`code` 和 `acp`）在运行时初始化前安装进程日志订阅器。警告和错误（包括 SQLx 慢查询警告）写入 stderr，不带 ANSI 样式；stdout 保留给命令结果和协议帧。默认不输出普通 DEBUG 查询日志。交互式产品仍自行管理终端输出。

独立 Host 将 stderr 捕获到所属 Root 的私有控制缓存文件 `host.stderr.log`：按需启动在创建进程前接好捕获文件，前台服务在选定 Root 后开始捕获。使用 `maka host logs --root-id ID` 读取，添加 `--follow` 持续接收 NDJSON，添加 `--after CURSOR` 从游标续读。每页以 Base64 保留原始字节，并报告尚无捕获文件、文件替换或截断；清理控制缓存也会删除这些日志。读取不要求安装系统服务。默认产品的“诊断 → Host 日志”无需会话即可读取和跟随日志。

## 产品更新、恢复与 URL 安装

组合命令读取完整的 JSON 来源和远程导出文件。插件组合编辑器按最多 64 KiB 的 UTF-8 分页加载指定来源版本，并基于该版本保存已确认的原生编辑器快照。整份文档没有独立字节上限；类型校验、包校验、Host 权限、版本冲突和实际资源准入仍然生效。已提交版本保留完整来源，供后续读取和重新打开 Root 使用。

更新独立 Host 已选的内置插件时，先停止该 Host，再指定它的 State Root：

```sh
maka application update-defaults --root /path/to/host
maka application update-defaults --root /path/to/host --apply
```

先检查预览中的具体包版本。执行更新会将组合中显式选择、且 ID 与当前可执行文件所带插件匹配的包替换为发行版本，包括使用相同 ID 的本地修改包。其他包、组合设置、权限和业务数据保留，不自动添加新的默认组件；组合校验失败时拒绝更新。中断后再次执行 `--apply` 完成同一次更新，再启动 Host。此命令更新应用包；`host upgrade` 更新原生可执行文件。

`update-defaults` 要求当前可执行文件能够解码已有的包记录。包含已移除内部格式（例如旧 `desktop` surface）的 Root 会在预览前失败；保留原 Root，另行初始化新的 Root。

恢复前先关闭产品 UI；恢复通过管理服务更新独立 Host。直接安装插件包时，所选 Root 必须处于未运行状态。两个命令默认使用 `~/.maka/application`；可用 `--root DIRECTORY` 选择其他安装。

```sh
maka application restore-defaults
maka application install-url --url https://example.com/plugin.maka-plugin
```

`restore-defaults` 从首次安装的原始版本恢复 Host、管理端和 UI 组合，使用当时的完整包和实例 ID。会话、凭据、插件数据、连接授权及源码历史仍保留。恢复的是该安装的初始组合，不是新版本可执行文件的默认值。中断后再次打开会继续同一组已准备的版本；若发现后续冲突修改则拒绝覆盖。

`install-url` 接受 HTTP(S) `.maka-plugin` 包的直接下载链接，限制下载大小和时长、校验整包摘要，并保存不可变版本。之后在插件组合编辑器中启用。可用 `--sha256 HEX` 验证发布者提供的包摘要、`--expected-revision N` 指定预期源码版本，或用 `--deployment UUID` 选择独立提交的部署。下载遵循进程的代理环境。运行中的插件安装页面和 Agent `UIRequest` 也通过绑定的部署管理器支持 URL 安装。
