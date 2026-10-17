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

# Native build tooling

[English](README.md)

这些脚本从指定源码构建原生包及 npm 启动器。对外构建与发布顺序见 [packages/cli](../../packages/cli/README.zh-CN.md)。

macOS 原生打包需要 Xcode 26 或更新版本。Linux 需要 GLib、X11/XCB、xkbcommon、Wayland、Fontconfig 和 FreeType 开发文件，以及 cargo-zigbuild 与 Zig。[原生 CI 工作流](../../.github/workflows/npm-publication.yml)记录构建环境。

Rust 构建需要 libclang 来生成 SQLite preupdate-hook 绑定，即使已有 V8 缓存包也需要。

发布包使用关闭可选 LGPL glibc 数学代码的固定 V8 构建。完成的库、绑定和 notices 缓存在 `target/v8` 或 `$CARGO_TARGET_DIR/v8`。缓存未命中时，构建器下载 [v8-prebuilts.json](v8-prebuilts.json)指定的包并校验摘要。缓存损坏或固定来源失效会直接报错；没有对应固定来源时才从源码构建。源码构建还需要 Python、Git、Clang/libclang 19 或更新版本、C++ 工具链，以及较多磁盘空间和时间。

缓存键标识 V8 源码、目标、特性和生成代码。新增键时，使用已完成原生工作流输出的精确归档，将其发布为带版本的依赖预发布资产，再把 URL、哈希和验证依据写入 manifest。保留已有资产及键的含义。每个包仍执行原生链接和 smoke 检查。

依赖变更后执行：

```sh
node scripts/rust/dependencies.mjs generate
node scripts/rust/notices.mjs generate
node scripts/rust/dependencies.mjs check
node scripts/rust/notices.mjs check
```

检查生成的清单和许可证文本。[notices-sources.json](notices-sources.json)记录已审查的上游例外。发布组装还包含实际 V8 构建与 Rust 标准库的 notices。TSV 清单不能替代完整许可证文本。

在 macOS/Linux 上比较直接启动 TUI 的耗时：先构建并保留前后两个可执行文件，再运行：

```sh
python3 scripts/rust/benchmark-startup.py --binary target/debug/maka \
  --baseline /tmp/maka-before --runs 5 --output /tmp/maka-startup
```

基准使用独立数据目录，首次安装单独计时，重开按前后版本交替执行。它等待首页出现并确认命令面板可用后退出。输出目录保留原始终端输出、失败次数及耗时中位数和范围。两版应使用相同构建配置，测量期间避免同时构建。

长任务对比使用两个版本保留的 release 测试可执行文件，基准夹具必须相同。
通过 `cargo test --release --no-run` 构建 `maka-event-log --test archive`
和 `maka-agent-engine --test integration`，然后运行：

```sh
python3 scripts/rust/benchmark-long-task.py --kind engine \
  --baseline /tmp/engine-before --candidate /tmp/engine-after \
  --sizes 250,1000 --runs 5 --output /tmp/maka-long-task
```

归档测试可执行文件使用 `--kind prune`；真实上下文压缩后的 Engine 测量使用
`--checkpoint`。脚本交替运行两个版本，检查实际完成的工作相同，保留原始日志、
错误、中位数和范围。记录构建 profile、目标架构和依赖 feature；feature unification
可能使独立测试与产品使用不同的哈希后端。使用 `sha2` 0.10 的旧 Engine 基线在受支持的
ARM64 平台上需加 `--features sha2/asm`，才能与产品后端一致。当前依赖在运行时选择
受支持的 CPU 指令，并保留软件回退。

造数据不计时；Engine 耗时涵盖请求送达与执行完成，不包含
Host IPC。[Engine 夹具](../../plugins/agent/engine/tests/integration/long_task_perf.rs)
和 [prune 夹具](../../platform/event-log/tests/archive/performance.rs) 提供工具稀疏、
工具密集及投影大小等参数。测量时不要同时构建；CPU 剖析与耗时对比分开运行。

测量 1,000 / 10,000 / 100,000 轮时，将 `MAKA_ENGINE_PERF_FIXTURE_DIR` 指向
系统临时目录内的一次性目录，并用 `MAKA_ENGINE_PERF_FIXTURE_TAG` 明确标记版本。
Engine 夹具走真实 append 校验，每 1,000 轮执行真实压缩；关闭后的 SQLite 快照
按全部负载参数及夹具源码哈希缓存，修改夹具会使缓存失效。对夹具源码一致的保留
可执行文件运行 `--kind engine --checkpoint --sizes 1000,10000,100000`。
造数据与请求耗时分开，前者可能远长于实际测量过程。

历史与取消测试使用 `--kind responsive`，并将 `MAKA_RESPONSIVENESS_FIXTURES`
设为上述一次性 `.sqlite` 快照路径，以逗号分隔。它验证冷 transcript 准备、真实
历史页，以及 admission 前、admission 完成后 10 ms 和模型 HTTP 请求到达后的取消。
admission 前覆盖普通与手动消息预检：10 ms 触发时 start 必须仍在等待，随后取消且
不产生 opening 或模型请求。夹具需足够大，提前完成 admission 会使测试失败。
admission 后的定时触发不保证引擎仍处于准备阶段，需核对模型请求计数。
停止计时截至 start 返回取消，或已 admission 的 Engine 完成清理且持久化取消结果；
不包含 EventLog 关闭时等待被放弃读取结束的时间。
这些是存储／Engine API 指标，不是 GUI 或 Host IPC 延迟。上下文与裁剪读取在
调用方取消后通过 SQLite progress handler 中断，并等事务清理完成后复用连接；
双 reader 回收及事务恢复另有行为测试，不代表任意同步 Rust 计算都可立即中断。

`--kind transcript --sizes 128,512` 使用保留的 release
`maka-event-log --test transcript_index` 可执行文件，补测同一 invocation 内的
大量模型步骤。每个样本从新副本生成 transcript，并逐条核对助手消息内容。

`--kind executor --sizes 128,512` 用同一 transcript 可执行文件测量外部执行器
的连续工具调用，并将全部持久化消息与 canonical replay 对比，覆盖工具进度、
结果和最终 invocation 状态。

要测量同一活跃 Host 会话中的多轮压缩，保留 release 构建的
`maka-agent-native --lib` 测试可执行文件，再运行：

```sh
MAKA_COMPACTION_CYCLES=10 MAKA_COMPACTION_PROMPT_BYTES=65536 \
  "$NATIVE" owner::tests::scale::compaction::real_host_same_session_compaction \
  --ignored --exact --nocapture --test-threads=1
```

每轮新增输入并通过真实 Host 回执执行手动压缩。最后一次请求验证最新 checkpoint
已被采用，分页历史核对原始消息。`MAKA_AGENT_COMPACTION` JSON 输出实际请求与
checkpoint 数量、已验证字节数及各操作的延迟／CPU 时间。操作计时排除初始化、
证据读取和关闭，但包含同进程内的受控 HTTP 模型。请求 JSON 大小是规范化后的
序列化长度；峰值 RSS 覆盖整个进程。该探针使用四个 Tokio worker，不测量 IPC、
渲染、自动压缩、模型推理或能耗。

要在 Unix 上测量真实服务工具执行，使用同一个 release native 测试可执行文件，每个进程
只运行一个 case：

```sh
MAKA_TOOL_STORM_CALLS=64 MAKA_TOOL_STORM_PAYLOAD_BYTES=128 \
  MAKA_TOOL_STORM_BACKEND=native \
  "$NATIVE" owner::tests::service_tools::storm::real_service_tool_storm \
  --ignored --exact --nocapture --test-threads=1
```

该 case 连续执行 mutation。改用同模块的 `real_service_query_fanout` 可测量
同一模型 step 中的只读调用；backend `javascript` 使用真实 JavaScript 服务。
fanout 数量不代表同时执行的 handler 数量。`MAKA_AGENT_TOOL_STORM` JSON 记录
实际调用、精确结果校验、请求、延迟／CPU、进程峰值 RSS 和清理结果。计时包含
受控 HTTP responder，不包含外部推理、IPC 或渲染。大 payload 可以触发真实自动
压缩：responder 返回 Summary 正文，验证生成的 checkpoint 及后续 Main 对它的
采用，同时通过历史逐一核对原始工具结果。Summary 请求和 checkpoint 与工具工作量
分别记录。这验证受控协议和生命周期，不证明真实模型的上下文容量或摘要质量。
只有完整执行且全部校验通过的工作才计为成功 case。
