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

# Maka NSH terminal shell

An experimental first-party NSH application over public Maka Host services. The native executable selects one existing Root and Agent session; JavaScript receives that connected client. The Host must expose the `agent`, `settings` and `receipts` endpoints.

For a Root whose Host is not running, start the existing service entrypoint in a separate terminal:

```sh
cargo run -p maka-cli --bin maka -- host serve --root /path/to/root
```

Then run the independent shell from the workspace root, selecting an existing session:

```sh
cargo run -p maka-terminal-shell -- --root /path/to/root --session SESSION_ID
```

The executable verifies root-bound discovery and the live Host epoch. The durable client journal is `<root>/clients/terminal.sqlite`; drafts and operation receipts are scoped by their original Host and provider identities. The existing product and release launchers remain unchanged.

Ctrl+Q requests exit independently of JavaScript and terminal output. SIGTERM does the same on Unix. The shell attempts terminal restoration, then closes its Host requests and native tasks; incomplete restoration or cleanup returns an error.

Draft recovery starts at the latest acknowledged saved revision. Text typed after that revision can be lost on close. A send uses the exact saved revision; ambiguous results remain available for receipt recovery rather than sending the same operation again.
