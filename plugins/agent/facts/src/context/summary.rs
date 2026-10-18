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
    () => {
        "## Checkpoint boundary\n[Last visible event or action, using a supplied reference if available. This brief describes the captured prefix; later messages may update it.]\n\n## Objective and constraints\n[User goal, scope, success criteria, constraints, authorization boundaries, corrections and unresolved requests as of this checkpoint.]\n\n## Work state\n[Separate verified completions and their evidence from work in progress, plans, failed attempts, blockers and unknown outcomes. Record tool success separately from goal completion.]\n\n## Decisions and attempts\n[Current decisions and reasons, relevant alternatives still open, failed approaches and why they failed. Identify superseded decisions when needed to avoid reviving them.]\n\n## Continuation context\n[Essential paths, symbols, IDs, commands, errors, evidence and result references, including exact cursors or offsets needed to resume reading. Pending user questions or approvals; issued but unsettled operations and running, background or delegated work with owner, original operation/call/cell/task/session IDs, last observed status and evidence, and result or recovery references. Keep unsettled requests pending unless captured evidence resolves them.]\n\n## Provisional next actions\n[First reconcile any later messages for results, user corrections and superseded actions so work already launched or completed is not replayed. Then give remaining actions in order, dependencies, and input or evidence needed to proceed or confirm completion.]"
    };
    (zh_cn) => {
        "## 检查点边界\n[最后可见的事件或行动；有现成引用时附上。此摘要描述所捕获的历史前缀，后续消息可能更新其中的状态。]\n\n## 目标与约束\n[截至此检查点的用户目标、范围、成功标准、约束、授权边界、纠正及尚未解决的要求。]\n\n## 工作状态\n[区分已验证完成的工作及其证据、进行中的工作、计划、失败尝试、阻碍和结果未知的操作。工具调用成功与目标完成分别记录。]\n\n## 决定与尝试\n[当前决定及理由、仍待考虑的相关选项、失败方案及原因。必要时标明已被取代的决定，避免重新采用。]\n\n## 续接所需上下文\n[关键路径、符号、ID、命令、错误、证据和结果引用，包括继续读取所需的精确游标或偏移量。待用户回答的问题或审批；已发出但尚未结束的操作，以及运行中、后台或已委派工作的负责人、原始操作/调用/cell/任务/会话 ID、最后观测的状态与证据、结果或恢复引用。没有捕获到解决证据的请求仍标为待处理。]\n\n## 暂定后续行动\n[先核对后续消息中的结果、用户纠正和已被取代的行动，避免重复发起已启动或已完成的工作。再按顺序列出剩余行动、依赖关系，以及继续推进或确认完成所需的输入或证据。]"
    };
    (zh_tw) => {
        "## 檢查點邊界\n[最後可見的事件或行動；有現成參照時附上。此摘要描述所擷取的歷史前綴，後續訊息可能更新其中的狀態。]\n\n## 目標與約束\n[截至此檢查點的使用者目標、範圍、成功標準、約束、授權邊界、修正及尚未解決的要求。]\n\n## 工作狀態\n[區分已驗證完成的工作及其證據、進行中的工作、計畫、失敗嘗試、阻礙和結果未知的操作。工具呼叫成功與目標完成分別記錄。]\n\n## 決定與嘗試\n[目前決定及理由、仍待考慮的相關選項、失敗方案及原因。必要時標明已被取代的決定，避免重新採用。]\n\n## 接續所需上下文\n[關鍵路徑、符號、ID、命令、錯誤、證據和結果參照，包括繼續讀取所需的精確游標或偏移量。待使用者回答的問題或核准事項；已發出但尚未結束的操作，以及執行中、背景或已委派工作的負責人、原始操作/呼叫/cell/任務/工作階段 ID、最後觀測的狀態與證據、結果或恢復參照。未擷取到解決證據的請求仍標為待處理。]\n\n## 暫定後續行動\n[先核對後續訊息中的結果、使用者修正和已被取代的行動，避免重複發起已啟動或已完成的工作。再依序列出剩餘行動、相依關係，以及繼續推進或確認完成所需的輸入或證據。]"
    };
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
            "Summarize the captured conversation above as a checkpoint brief for another assistant. Compression runs asynchronously; messages produced after capture will follow the brief when work continues. Output only the brief, using the user's language. Merge previous checkpoints with the captured conversation: apply later user corrections and new evidence while retaining unresolved requirements. Distinguish source facts, reported claims, inferences and missing information. Omitted or truncated history is unavailable evidence, not proof that something did not happen. Preserve the exact references needed to continue; condense repetition and logs into relevant findings. Use these sections, adapting their detail to the work:\n\n",
            summary_format!()
        ),
        Language::ZhCn => concat!(
            "将以上捕获的对话总结为供另一个助手使用的检查点摘要。压缩异步进行；捕获后产生的消息会在工作继续时接于摘要之后。仅输出摘要，使用用户的语言。将先前检查点与所捕获的对话合并：纳入用户后续纠正和新证据，保留尚未解决的要求。区分来源事实、转述的声称、推测和缺失信息。被省略或截断的历史是证据不可用，不代表事情没有发生。精确保留继续工作所需的引用，将重复内容和日志归纳为相关发现。按以下章节组织，根据工作需要调整详略：\n\n",
            summary_format!(zh_cn)
        ),
        Language::ZhTw => concat!(
            "將以上擷取的對話總結為供另一個助手使用的檢查點摘要。壓縮非同步進行；擷取後產生的訊息會在工作繼續時接於摘要之後。僅輸出摘要，使用使用者的語言。將先前檢查點與所擷取的對話合併：納入使用者後續修正和新證據，保留尚未解決的要求。區分來源事實、轉述的聲稱、推測和缺失資訊。被省略或截斷的歷史是證據無法取得，不代表事情沒有發生。精確保留繼續工作所需的參照，將重複內容和日誌歸納為相關發現。按以下章節組織，依工作需要調整詳略：\n\n",
            summary_format!(zh_tw)
        ),
    }
}
