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

# maka-workspace-api

[English](README.md)

定义工作区文件、进程、终端、协议进程和 Jev 的共用契约。原生提供方与开发工具使用相同声明。请求路径和工作区标识用于描述选择；提供方仍需检查实际绑定目录和调用权限。

Terminal 契约提供 `readOutput` 和 `watchOutput`，完整保留原 live resource 的有序 PTY 字节（Base64）及原生尺寸变化。每个数据事件最多 16 KiB，资源持有期间保留全部事件。`after` 必须与原始目标一起保留。新 parser 直接订阅 `watchOutput(after: 0)`，按首个保留事件的 `initialSize` 初始化一次，再依序处理全部 Data/Resize；状态完整的 parser 可从已知序号继续。已取得的 reader 可在 worker 退休后排完数据，但退休或 Host 重启后的新接入返回 `Unavailable`；先前的 `readOutput` 不为后续请求保活。真实 replay I/O 错误使调用失败。纯文本屏幕不是完整 parser checkpoint。`Closed` 只表示字节源关闭，不代表进程清理已确认。输出观察需要显式方法权限，不需要控制器。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
