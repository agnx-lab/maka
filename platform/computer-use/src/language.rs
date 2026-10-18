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

//! Invocation-owned copy for CUA documentation and recovery guidance.
use maka_agent_facts::language::Language;

pub(crate) fn documentation(language: Language) -> &'static str {
    match language {
        Language::En => include_str!("api.md"),
        Language::ZhCn => include_str!("api.zh-CN.md"),
        Language::ZhTw => include_str!("api.zh-TW.md"),
    }
}

pub fn reset(language: Language) -> &'static str {
    match language {
        Language::En => "Computer Use REPL reset. Bind targets again.",
        Language::ZhCn => "Computer Use REPL 已重置。请重新绑定目标。",
        Language::ZhTw => "Computer Use REPL 已重設。請重新綁定目標。",
    }
}
pub fn partial_observations(language: Language) -> &'static str {
    match language {
        Language::En => {
            "\nObservations emitted before failure (actions may already have completed):\n"
        }
        Language::ZhCn => "\n失败前输出的观察（操作可能已经完成）：\n",
        Language::ZhTw => "\n失敗前輸出的觀察（操作可能已經完成）：\n",
    }
}
pub fn closed(language: Language) -> &'static str {
    match language {
        Language::En => {
            "\nThis REPL is closed. Use cua_reset, rebind and observe before further actions; do not replay uncertain input."
        }
        Language::ZhCn => {
            "\n此 REPL 已关闭。请先调用 cua_reset，重新绑定并观察，再继续操作；不要重放不确定的输入。"
        }
        Language::ZhTw => {
            "\n此 REPL 已關閉。請先呼叫 cua_reset，重新綁定並觀察，再繼續操作；不要重播不確定的輸入。"
        }
    }
}
pub(crate) fn uncertain(language: Language) -> &'static str {
    match language {
        Language::En => {
            "The driver did not verify the requested effect. Read getAXState() or getScreenshot() before deciding what to do next; do not replay an uncertain action."
        }
        Language::ZhCn => {
            "驱动未验证请求的效果。请先读取 getAXState() 或 getScreenshot()，再决定下一步；不要重放不确定的操作。"
        }
        Language::ZhTw => {
            "驅動程式未驗證請求的效果。請先讀取 getAXState() 或 getScreenshot()，再決定下一步；不要重播不確定的操作。"
        }
    }
}

pub(crate) fn inactive(language: Language) -> &'static str {
    match language {
        Language::En => {
            "Computer Use session is no longer active. Call cua_reset, then bind and observe the current app or tab before acting."
        }
        Language::ZhCn => {
            "Computer Use 会话已失效。请调用 cua_reset，重新绑定并观察当前应用或标签页，再继续操作。"
        }
        Language::ZhTw => {
            "Computer Use 工作階段已失效。請呼叫 cua_reset，重新綁定並觀察目前應用程式或分頁，再繼續操作。"
        }
    }
}
pub(crate) fn terminal(language: Language) -> &'static str {
    match language {
        Language::En => {
            "\nCall cua_reset, then bind and observe the current app or tab before acting. This refused operation did not dispatch input."
        }
        Language::ZhCn => {
            "\n请调用 cua_reset，重新绑定并观察当前应用或标签页，再继续操作。此次被拒绝的操作未派发输入。"
        }
        Language::ZhTw => {
            "\n請呼叫 cua_reset，重新綁定並觀察目前應用程式或分頁，再繼續操作。此次被拒絕的操作未派發輸入。"
        }
    }
}
pub(crate) fn background_unavailable(language: Language) -> &'static str {
    match language {
        Language::En => {
            "Background input is unavailable for this control on the current desktop. Use an indexed accessibility action if one is exposed by app.getAXState(); otherwise this action requires a supported desktop input backend. The public Cua API cannot switch this call to foreground delivery."
        }
        Language::ZhCn => {
            "当前桌面上的此控件不支持后台输入。若 app.getAXState() 提供了索引无障碍操作，请使用该操作；否则需要支持此输入的桌面后端。公开 Cua API 无法将此次调用切换为前台交付。"
        }
        Language::ZhTw => {
            "目前桌面上的此控制項不支援背景輸入。若 app.getAXState() 提供了索引輔助使用操作，請使用該操作；否則需要支援此輸入的桌面後端。公開 Cua API 無法將此次呼叫切換為前景交付。"
        }
    }
}

// CDP preserves the original error and uncertainty class; the accepted browser
// dispatch owns this single no-replay instruction for every unknown outcome.
pub(crate) fn browser_unknown(language: Language) -> &'static str {
    match language {
        Language::En => "; do not replay a mutation",
        Language::ZhCn => "；不要重放可能产生变更的操作",
        Language::ZhTw => "；不要重播可能產生變更的操作",
    }
}
