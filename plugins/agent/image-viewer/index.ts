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

import type { Component } from '../../../packages/plugin-sdk/src/index.js';
import { imageViewerContract, type ImageViewerProps } from '../../shared/contracts.js';
import { copy } from './locale.js';
import { output, type Presentation, type Node } from '../../shared/view.js';

interface State {
  revision: number;
}

export function imageViewer<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ImageViewerProps, N> {
  const frame = (state: State, props: ImageViewerProps, closed = false) => {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale);
    return output(
      state,
      ui.box(
        'image-viewer',
        [
          ui.box(
            'image-toolbar',
            [
              ui.button('image-back', text.sourceBack),
              ui.text('image-title', text.imageTitle, props.theme.text, 600),
            ],
            { row: true },
          ),
          ui.text('image-terminal', text.imageTerminal, props.theme.muted),
        ],
        { visible: props.image !== null, padding: 1, terminal: { grow: 1 }, scroll: 'manual' },
      ),
      closed ? [{ name: 'closed', value: {} }] : [],
    );
  };
  return {
    mount(props) {
      return frame({ revision: 0 }, props);
    },
    update(state, props) {
      return frame({ revision: state.revision + 1 }, props);
    },
    event(state, input, props) {
      state.revision++;
      return frame(state, props, input.kind === 'raw' && input.key === 'image-back');
    },
  };
}
export function activate() {
  return {
    ImageViewer: { kind: 'component', contract: imageViewerContract, component: ['component'] },
  };
}
