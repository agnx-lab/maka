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

# maka-files-native

[English](README.md)

为显式选择的工作区提供文件操作。`provide` 注册服务，`attachments` 处理捕获的文件资源。包装层向文件引擎传递原始调用作用域与目录权限。文件工具与命令共用会话沙箱策略：允许读取工作区外文件；workspace-write 默认允许写入工作区及系统临时目录，其他写入需要权限审批。所选目录仍通过捕获的句柄访问，审批不能绕过工作区元数据与 Host 私有路径保护。

`readText` 也可分页读取调用方 Session 可见的上传附件与原始工具结果资源。已选择的 Read 工具使用当前活跃 Invocation 的权限；资源地址和续页地址不会授予其他 Session 的访问权。每个文本附件页都会加载已受理上传的完整内容，验证 UTF-8 且不含 NUL 字节后，再返回大小受控的页面。归档读取复用原始模型投影，不会再次执行源工具。当前每个归档页仍会先加载完整原始投影，取消也会等待该存储操作结束。

[源码](src/lib.rs) · [Cargo 配置](Cargo.toml)
