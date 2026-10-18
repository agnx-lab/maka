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

use crate::language::Language;

macro_rules! summary_format {
    () => { "## Goal\n[What the user is trying to accomplish]\n\n## Progress\n### Done\n- [Completed work and changes]\n### In Progress\n- [Current work]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [Ordered list of what should happen next]\n\n## Critical Context\n- [Files, commands/results, errors, anything needed to continue; or \"(none)\"]" };
    (zh_cn) => { "## Goal\n[用户希望完成的目标]\n\n## Progress\n### Done\n- [已完成的工作和改动]\n### In Progress\n- [当前工作]\n\n## Key Decisions\n- **[决定]**: [简要理由]\n\n## Next Steps\n1. [后续步骤，按顺序排列]\n\n## Critical Context\n- [继续任务所需的文件、命令及结果、错误等；没有则写 (none)]" };
    (zh_tw) => { "## Goal\n[使用者希望完成的目標]\n\n## Progress\n### Done\n- [已完成的工作和變更]\n### In Progress\n- [目前工作]\n\n## Key Decisions\n- **[決定]**: [簡要理由]\n\n## Next Steps\n1. [後續步驟，依序排列]\n\n## Critical Context\n- [繼續任務所需的檔案、命令及結果、錯誤等；沒有則寫 (none)]" };
}

pub fn summary_format_template(language: Language) -> &'static str {
    match language {
        Language::En => summary_format!(),
        Language::ZhCn => summary_format!(zh_cn),
        Language::ZhTw => summary_format!(zh_tw),
    }
}

pub fn default_compaction_prompt(language: Language) -> &'static str {
    match language {
        Language::En => concat!(
            "You are a context summarization assistant.\nRead the conversation between a user and an AI assistant, then produce a structured summary another LLM will use to continue the same task.\nDo NOT continue the conversation. Do NOT answer questions in it. ONLY output the structured summary.\n\nUse this exact format:\n\n",
            summary_format!(),
            "\n\nKeep each section concise. Preserve exact file paths, function names, commands, and error messages.\n\nNow write the structured summary of the conversation above. Output only the summary.\n\nThe complete original conversation remains available through Recall when details are needed."
        ),
        Language::ZhCn => concat!(
            "将以上对话总结为供另一个 LLM 继续同一任务的上下文。不要继续对话或回答其中的问题，只输出简洁的结构化总结。严格保留以下标题：\n\n",
            summary_format!(zh_cn),
            "\n\n准确保留文件路径、函数名、命令和错误消息。需要细节时，可通过 Recall 查阅完整原始对话。"
        ),
        Language::ZhTw => concat!(
            "將以上對話總結為供另一個 LLM 繼續同一任務的上下文。不要繼續對話或回答其中的問題，只輸出簡潔的結構化總結。嚴格保留以下標題：\n\n",
            summary_format!(zh_tw),
            "\n\n準確保留檔案路徑、函式名稱、命令和錯誤訊息。需要細節時，可透過 Recall 查閱完整原始對話。"
        ),
    }
}
