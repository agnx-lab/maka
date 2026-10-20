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

# apply-patch

[English](README.md)

解析完整补丁文本，将更新应用到传入字符串，保留操作顺序、原始上下文和换行符。准备阶段在内存中保留完整字符串及解析后的行，不执行文件系统 I/O。调用方需要先校验路径并取得写入权限，再将准备好的操作应用到文件。上游来源与改动记录见 [SOURCE.md](SOURCE.md)。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
