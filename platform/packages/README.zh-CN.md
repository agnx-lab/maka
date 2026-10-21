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

# maka-packages

[English](README.md)

从声明和文件加载不可变应用包，在组合规划前校验路径、内容和 manifest。包引用标识具体内容；加载或安装内容不会启动实例，也不会创建绑定。入口是 `Package` 和 `manifest` 模块。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)

目录加载与包文件保留全部普通文件及其原始字节，包括二进制资源和本地化展示。内容摘要、可移植路径、不跟随符号链接的目录句柄和不覆盖的导出发布共同绑定完整包；分配与文件系统失败仍明确报告。
