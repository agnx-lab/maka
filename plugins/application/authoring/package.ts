/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

import * as sdk from '../../../packages/plugin-sdk/src/index.js';
import { native } from '../../shared/package.js';
export default {
  schemaVersion: 1,
  id: 'maka.application.authoring',
  displayName: 'App authoring',
  description: 'Create and validate plugin apps for Maka.',
  localizations: {
    'zh-CN': { displayName: '应用开发', description: '创建和验证 Maka 插件应用。' },
  },
  application: {
    entries: {
      host: native(sdk.nativeImplementations.uiAuthoring),
    },
  },
};
