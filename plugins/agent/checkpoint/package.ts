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
  id: 'maka.agent.checkpoint',
  displayName: 'Saved view state',
  description: 'Restore navigation and drafts after reopening a view.',
  localizations: {
    'zh-CN': { displayName: '页面状态保存', description: '重新打开页面时恢复导航位置和草稿。' },
  },
  application: {
    entries: {
      tui: native(sdk.nativeImplementations.valueCheckpoint),
    },
  },
};
