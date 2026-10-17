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

import * as own from '../../shared/contracts.js';
import { uiEntries } from '../../shared/package.js';

const entries = uiEntries({
  contract: own.imageViewerContract,
  export: 'ImageViewer',
  imports: {},
});

export default {
  schemaVersion: 1,
  id: 'maka.agent.image-viewer',
  displayName: 'Image viewer',
  description: 'Open images attached to a conversation.',
  localizations: {
    'zh-CN': { displayName: '图片查看器', description: '打开对话中的图片附件。' },
  },
  application: { entries },
};
