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

# LLRT RSA dependency compatibility

This is `modules/llrt_crypto` from [LLRT revision
7b95c82a9b15e7ddfb2778eca4b5a63111e74f51](https://github.com/awslabs/llrt/tree/7b95c82a9b15e7ddfb2778eca4b5a63111e74f51/modules/llrt_crypto),
the revision already used by Maka's GPUI shell standard library. The upstream
Apache-2.0 `LICENSE`, `NOTICE` and the package's declared `README.md` are copied
from that repository's root. `upstream.json` records the original finite file
inventory and SHA-256 hashes before changes.

The sole source change backports [upstream commit
a684efb8994d433a00c949fe698f982f8f613f90](https://github.com/awslabs/llrt/commit/a684efb8994d433a00c949fe698f982f8f613f90):
seven RSA DER parsing calls name their existing `UintRef` integer representation.
`rsa` 0.10.0-rc.19 makes that representation generic, so the previous calls no
longer infer it. The changed source file matches the official upstream fix
byte for byte. The RSA key representation and crypto operations remain the same.

The manifest preserves every feature and dependency setting. Its six LLRT
runtime path dependencies and its `llrt_test` development dependency are anchored
to the same original Git revision, since their original sibling paths do not
exist in this directory. The existing `rquickjs` facade continues to select the
GPUI shell's original VM. No other LLRT sources or Buffer class identities are
updated. Remove this patch when that standard library adopts the upstream fix.
