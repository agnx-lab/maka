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

import { Canvas } from 'nsh';

export function Icon(props: {
  name:
    | 'back'
    | 'sidebar'
    | 'plus'
    | 'send'
    | 'stop'
    | 'close'
    | 'down'
    | 'right'
    | 'search'
    | 'settings'
    | 'folder';
  color: string;
}) {
  return (
    <Canvas
      pointerEvents="none"
      style={{ width: 16, height: 16, flexShrink: 0 }}
      draw={(cx) => {
        cx.strokeStyle = props.color;
        cx.fillStyle = props.color;
        cx.lineWidth = 1.5;
        cx.lineCap = 'round';
        cx.lineJoin = 'round';
        cx.beginPath();
        switch (props.name) {
          case 'sidebar':
            cx.roundRect(1.5, 2.5, 13, 11, 2);
            cx.moveTo(6, 2.5);
            cx.lineTo(6, 13.5);
            break;
          case 'plus':
            cx.moveTo(8, 3);
            cx.lineTo(8, 13);
            cx.moveTo(3, 8);
            cx.lineTo(13, 8);
            break;
          case 'send':
            cx.moveTo(8, 13);
            cx.lineTo(8, 3);
            cx.moveTo(3.5, 7.5);
            cx.lineTo(8, 3);
            cx.lineTo(12.5, 7.5);
            break;
          case 'stop':
            cx.roundRect(4, 4, 8, 8, 1);
            cx.fill();
            return;
          case 'close':
            cx.moveTo(4, 4);
            cx.lineTo(12, 12);
            cx.moveTo(12, 4);
            cx.lineTo(4, 12);
            break;
          case 'down':
            cx.moveTo(4, 6);
            cx.lineTo(8, 10);
            cx.lineTo(12, 6);
            break;
          case 'back':
            cx.moveTo(10, 4);
            cx.lineTo(6, 8);
            cx.lineTo(10, 12);
            break;
          case 'right':
            cx.moveTo(6, 4);
            cx.lineTo(10, 8);
            cx.lineTo(6, 12);
            break;
          case 'search':
            cx.arc(7, 7, 4.5, 0, Math.PI * 2);
            cx.moveTo(10.5, 10.5);
            cx.lineTo(14, 14);
            break;
          case 'settings':
            cx.arc(8, 8, 5, 0, Math.PI * 2);
            cx.moveTo(10, 8);
            cx.arc(8, 8, 2, 0, Math.PI * 2);
            for (let i = 0; i < 8; i++) {
              const angle = (i * Math.PI) / 4;
              cx.moveTo(8 + Math.cos(angle) * 5, 8 + Math.sin(angle) * 5);
              cx.lineTo(8 + Math.cos(angle) * 7, 8 + Math.sin(angle) * 7);
            }
            break;
          case 'folder':
            cx.moveTo(1.5, 5);
            cx.lineTo(1.5, 3);
            cx.lineTo(6, 3);
            cx.lineTo(8, 5);
            cx.lineTo(14.5, 5);
            cx.lineTo(14.5, 13);
            cx.lineTo(1.5, 13);
            cx.closePath();
        }
        cx.stroke();
      }}
    />
  );
}
