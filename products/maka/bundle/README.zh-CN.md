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

# Product bundle

[English](README.md)

本目录选择 Maka 分发的包，并将其绑定成 Agent、WorkHub 或 Studio 组合。插件声明与实现位于 [plugins](../../../plugins/README.zh-CN.md)。

- [package-inputs.json](package-inputs.json)选择参与构建的包。
- [src/composition.ts](src/composition.ts)构造实例、绑定和安装身份。
- [scripts/build.mjs](scripts/build.mjs)将包内容与组合生成代码输出到 `dist`。

## 运行与开发

在仓库根目录运行：

```sh
just run
just run --root /path/to/business-host
```

`just run` 打开 [NSH 终端工作台](../terminal-shell/README.zh-CN.md)，默认使用 `~/.maka/application/host`。它连接已有 Host，不存在则自动启动。下文的编写命令独立生成和管理插件组合。

自定义 UI 产品可在关闭 Maka 后运行 `maka application update-defaults`，预览“上次默认组合、当前自定义组合、新版默认组合”的三方合并。没有冲突时加 `--apply` 执行同步；`--root PATH` 可选择其他安装。独立修改按字段合并；同一值、删除或有序列表发生冲突时，输出 JSON Pointer 路径，整次更新不写入。预览会校验合并后的组合和实际插件包。更新会保留已安装但未被组合选中的包，且保持它们所属的 Host 或 UI。同步完成后记录新版默认基线；中断的写入会在重新启动时继续完成。没有默认基线的 UI 安装需要显式更新组合。独立 Host 保留现有组合，无需 UI 或默认基线即可[更新已选插件包](../cli/README.zh-CN.md#产品更新恢复与-url-安装)。

`just product` 重建 TypeScript bundle。开发二进制读取 `dist` 或 `MAKA_PRODUCT_BUNDLE` 指定的目录，仅修改 TypeScript 无需重新编译 Rust。发布二进制嵌入 bundle。构建根据组合生成代码及具名包的精确字节生成 `product-version.txt`；未变化的启动只读取该标识，不扫描包内容，安装与更新仍校验实际包字节是否匹配。`dist` 和 `MAKA_PRODUCT_BUNDLE` 都是生成产物：修改源文件后重新构建，不要直接编辑输出文件。标识缺失、格式错误或内容不匹配时需要重新构建。需要与开发构建同时生成产物时，使用独立输出目录：

```sh
node products/maka/bundle/scripts/build.mjs --output /tmp/maka-product-bundle
```

## 生成安装配置

CLI 可以生成经过校验的配置和包，运行时无需 Node：

```sh
maka application compose --configuration installation.json --output product-source
```

输出目录必须不存在。命令生成 Host、管理服务和 UI 配置、连接授权、完整安装身份及 `.maka-plugin` 包，不安装或启动它们。

`installation.json` 选择 `presentation`（`tui` 或 `desktop`）、`locale`、`scheme`、`hostRoot` 和服务配置。Agent 使用生成的 `AgentConfiguration`；WorkHub 另外配置独立 `coordinator`。可选服务和 ACP 执行器需要显式选择。支持字段见[配置解析器](src/composition.ts)和 [SDK 类型](../../../packages/plugin-sdk/src/generated/runtime.ts)。

重新生成已有安装时，保留输出的 `keys`、`hostDeployment` 和 `managementDeployment`，它们标识已有所有者及数据。包修订标识内容，可以独立变化。

显式安装时，先用 `maka host init` 初始化目标 Host Root，再用 `maka application commit` 提交 `host.json`，提供部署 ID、预期修订和导出的包。组合及来源命令也支持独立管理的 UI 部署。各命令的 `--help` 列出必要参数。

提交只保存预期来源和包内容，不激活或选择该部署。显式部署服务、端点和原生授权的参数见 `maka application serve --help`。`maka --root HOST_ROOT` 和 `maka desktop --root HOST_ROOT` 使用 Host Root 保存的选择。

## 验证修改

```sh
npm --workspace @maka-agent/product run typecheck
npm --workspace @maka-agent/product test
MAKA_PRODUCT_CLI_CHECK=1 npm --workspace @maka-agent/product test
```

测试只构建一次，再执行保留的组件行为检查。设置 `MAKA_PRODUCT_CLI_CHECK` 后，还会使用实际 CLI 检查包和组合。原生渲染、焦点和输入变更需要在目标呈现端验证。
