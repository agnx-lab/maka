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

# maka-render-terminal

[简体中文](README.zh-CN.md)

Presents checked UI compositions in a terminal using Ratatui and Crossterm. `application` drives input, committed frames, native editors and teardown; `markdown` renders text content. The product supplies its composition and services. This renderer does not select Agent pages or embed JavaScript execution.

[Source](src/lib.rs) · [Cargo manifest](Cargo.toml)

The foreground loop wakes for input, work completion and actual visible deadlines. Input, changed recipes and native observations request a frame; visible animations and elapsed labels schedule their next frame. An unchanged view does not repeat layout, buffer diffing or terminal output.

Each native owner retains measurements and relative stack geometry for pure nodes in its immutable scene recipe. Queries are retained on demand for the current and preceding actual measurement pass; unused constraints retire instead of accumulating resize history. Replacing the recipe releases its memo. Components and adaptive panes keep dynamic measurement. Derived geometry does not consume native input or publication capacity; cache misses use ordinary layout. Native editors retain the two text widths used for content and scrollbar layout. These caches never publish input geometry or bypass the original flush checks.

The sole bounded output owner writes complete ANSI buffers without blocking foreground input. Encoding advances Ratatui's diff baseline; the foreground issues no further diff until that buffer has been fully written and flushed. Only that completion promotes the corresponding hit geometry; it is not a terminal display acknowledgement. A partially written buffer cannot be discarded. If a newer business frame commits meanwhile, the written older frame supplies only the actual screen geometry: it cannot restore retired callbacks or clear the newer frame's focus, opening acknowledgement or redraw demand. Stale business input is rejected against its original identity.

Required fallible native preparation precedes atomic logical publication. Each mount fixes its native recipe implementation type; implementation changes use a new child key/remount. Pure text parsing runs as bounded background preparation, and rejected or obsolete results preserve the committed document. Native observations use the shared bounded publication lifecycle; enqueueing does not acknowledge a raw command receipt.
