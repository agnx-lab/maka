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

# Plugins

[English](README.md)

叶目录中的 `package.ts` 声明一个插件，列出可执行入口、配置、导入和导出。Cargo crate 是编译单元：一个插件可以使用多个 crate，相关插件也可以共用实现 crate。

| 插件族 | 职责 |
| --- | --- |
| [agent](agent/README.zh-CN.md) | 会话、模型执行、工具、目标和调度 |
| [workspace](workspace/README.zh-CN.md) | 文件、进程、Web 和 Jev 服务 |
| [settings](settings/native/README.zh-CN.md) | 连接、凭据和偏好 |
| [skills](skills/native/README.zh-CN.md) | 技能发现、读取和发布 |
| [interactions](interactions/native/README.zh-CN.md) | 问题和表单 |
| [workhub](workhub/core/README.zh-CN.md) | 任务协调和 worker 结果 |
| `ui/` | 原生 UI 能力声明 |
| `application/` | 部署管理、插件安装与 UI 编写 |
| `shared/` | 共用 TypeScript 契约与开发辅助函数 |

修改已有插件时，从声明入手，沿所选实现查找代码。JavaScript 入口指定本地模块，例如由 `tui.ts` 构建的 `tui.js`。原生入口选择可执行文件提供的实现。插件族目录和已链接 crate 都不会自动激活插件。[桌面 shell](../products/maka/desktop-shell/README.zh-CN.md) 单独编写 app，并连接相同的公共 Host 服务。

插件族表示业务归属，不是互斥的 UI/服务分类。以 `package.ts` 为准：`host`、`tui` 入口说明实现运行位置，导出契约区分组件、服务和值。例如 `application/authoring` 同时包含 Host 服务与前台 UI worker。`ui/` 存放通用展示能力，并不收纳所有带 UI 的插件。大型组件可以按职责拆成普通模块；[AgentScreen](agent/agent-screen/README.zh-CN.md) 保留一个组件状态和生命周期，将命令、结果、事件及视图区域分开组织。

在仓库根目录构建单个包：

```sh
node scripts/plugins/build.mjs   --plugin plugins/workspace/workspace-web --output /tmp/maka-web-bundle
```

构建器会替换输出目录，包内容位于 `packages/`。可运行 `maka application inspect-package --package /tmp/maka-web-bundle/packages/workspace-web` 检查结果。

将插件加入 Maka 时，在[产品选择](../products/maka/bundle/package-inputs.json)中添加路径，再在[组合](../products/maka/bundle/src/composition.ts)中添加实例和绑定。新增原生实现还需在[产品注册表](../products/maka/native/README.zh-CN.md)中加入工厂。导入声明依赖，Host 授权决定可以执行哪些调用。

[插件开发 skill](../skills/maka-plugin-authoring/SKILL.md)提供可运行的独立示例，以及服务、UI、状态和原生扩展指南。[SDK](../packages/plugin-sdk/README.zh-CN.md)提供精确契约。

较大的 UI 组件（`application/apps`、`application/deployment`、`agent/scheduler`、`agent/conversation-import`、`settings/insights`、`settings/connection-form`）用 `state.ts` 定义共享状态和局部值类型，`operations.ts` 处理服务意图与结果结算，`view.ts` 构造场景，`events.ts` 处理输入，`index.ts` 负责组件创建和生命周期连接。这些仍是同一组件的普通模块，共用一个状态和发布边界，不增加事件总线或独立 store。小组件可以保留单文件。

插件列表使用作者在 `package.ts` 中提供的 `displayName` 和 `description`；用一句话说明用户能做什么。包 ID 继续作为稳定技术标识。可选的 `localizations` 按语言标签提供翻译，例如：

```ts
{
  id: 'example.notes',
  displayName: 'Notes',
  description: 'Write and find your notes.',
  localizations: {
    'zh-CN': { displayName: '笔记', description: '记录和查找笔记。' },
  },
  // application: ...
}
```

插件管理会展示并搜索这些说明，添加应用时也会展示。缺少翻译时使用默认文案，最后回退到实例名或包 ID。名称应让人预知操作结果或目的地，参照 [Apple 的清晰命名指南](https://developer.apple.com/videos/play/wwdc2026/290/)。常见操作使用带无障碍名称和提示的图标；重要或不熟悉的操作保留明确文字。长列表通过阅读区域的滚动观察加载相邻数据并保留可见行，数据游标不暴露为手动翻页。
