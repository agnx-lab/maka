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

# Native Shell (NSH)

This directory vendors the complete tracked source of
[`M4n5ter/nsh`, `tui/terminal-backend`](https://github.com/M4n5ter/nsh/tree/tui/terminal-backend)
at `de176695240d550f2b303643b8019146c7989e7f`. The upstream project is
[`longbridge/nsh`](https://github.com/longbridge/nsh).

`upstream.json` records the imported revision and SHA-256 of every imported file.
The source, tests, resources, original licenses and independent workspaces are
retained verbatim. `SOURCE.md` and `upstream.json` are Maka's provenance files.
Maka's workspace uses these local crates, including NSH's offscreen renderer and
image codec; changing them here changes the code built and tested with Maka.

Develop and verify changes in this tree together with their Maka callers. Send
reusable changes to `longbridge/nsh`. When synchronizing the fork or upstream,
review local differences against the recorded revision, use upstream APIs and
recommended patterns where appropriate, and remove patches that upstream has
superseded. Refresh the recorded source hashes to the newly imported revision;
they describe the import, not later Maka modifications.
