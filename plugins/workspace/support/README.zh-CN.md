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

# maka-workspace-support

[English](README.md)

在工作区服务之间共享目录选择与校验。它打开已配置根目录，排除 Host 私有路径，并检查所选工作区是否属于这些根目录。主要入口是 `Root`、`query` 和 `validate`。所选路径只能在所属服务提供的权限范围内使用。

子目录查询先枚举所选目录，再排序并分页返回；每页最多 32 个子目录，且不超过 48 KiB。普通文件不会阻止子目录浏览。使用返回的 `after` 游标继续查询；每次查询都会重新校验已发布根目录和所选目录的身份。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
