# Design: AI 提问 / elicitation 对话框

需求见 `prd.md`，codeg 事实见 `../../10-13-acp-claude-codex/research/codeg-acp-stream-ui.md` §3，
协议形状核对自 codeg `src-tauri/src/acp/question.rs:1255,1600` 与 `connection.rs:7951`。
桥接规范：`.trellis/spec/frontend/tauri-bridge.md`（ACP 段）。

## 1. 协议形状（已核对）

**请求**（server→client，`elicitation/create`）:
```json
{
  "jsonrpc": "2.0", "id": <id>, "method": "elicitation/create",
  "params": {
    "message": "<整段提示文本>",
    "mode": {
      "type": "form",
      "requestedSchema": {
        "type": "object",
        "properties": {
          "<fieldId>": { "type": "string", "title": "...", "enum": ["a","b"], "enumNames": [...] },
          "<fieldId2>": { "type": "array", "items": {"enum":[...]}, "title": "..." },
          "<fieldId3>": { "type": "string", "title": "..." }
        }
      },
      "scope": { "toolCallId": "..." }
    }
  }
}
```
> 字段名以实测为准（codex-acp 2.2.2）。实现时用**宽容解析**：`properties` 逐字段取
> `type` + `title`/`description` + `enum`/`enumNames`/`items.enum`；无法识别不 panic。

**响应**（client→server）:
```json
{ "jsonrpc": "2.0", "id": <id>, "result": { "action": "accept", "content": { "<fieldId>": <value> } } }
```
- `value` 按字段类型：single/text → `string`；multi → `string[]`。
- 拒答/无法渲染 → `{ "action": "decline" }`（agent 自行判断，不 hang）。

## 2. 现状与缺口（`src-tauri/src/acp.rs`）

- `handle_client_request`（`:331`）：只特判 `session/request_permission`；其余
  `fs/`、`terminal/`、`elicitation/` 一律 `-32601`（`:364-366`）。→ codex 提问静默失败。
- `initialize` 能力声明：不声明任何 client capability（D2 现状）。→ 需按引擎条件声明
  `elicitation.form`（仅 codex）。
- pending 机制：`conn.pending_perm: Mutex<HashMap<String,(Value, String)>>`
  （request_id → (rpc_id, session_id)），`acp_respond_permission` 回写。→ 新建一个**平行的**
  `pending_question` map（同一模式），不复用 perm（语义不同）。

## 3. Rust 设计

### 3.1 纯函数分类（新模块 `src-tauri/src/acp_question.rs`）

```rust
pub enum QuestionKind { Single, Multi, Text }

pub struct QuestionOption { pub value: String, pub label: String }

pub struct Question {
    pub id: String,          // = requestedSchema.properties 的键（回传 content 的 key）
    pub prompt: String,      // title | description | message
    pub kind: QuestionKind,
    pub options: Vec<QuestionOption>,   // Single/Multi 有；Text 空
}

pub enum ElicitationPlan { Questions(Vec<Question>), Decline }

/// 纯函数：从 elicitation params 提取问题；无法识别 → Decline。
/// 本期只认 form 模式 + single/multi/text；boolean/number/integer/URL → Decline。
pub fn classify_elicitation(params: &serde_json::Value) -> ElicitationPlan;

/// 纯函数：把用户答案映射成响应 result。
/// answers: Vec<(fieldId, Vec<String>)>；declined=true → decline。
pub fn build_elicitation_result(plan: &[Question], answers: &[(String, Vec<String>)], declined: bool)
    -> serde_json::Value;
```

- `classify_elicitation`：读 `params.mode.requestedSchema.properties`（缺失/非 object →
  `Decline`）；逐字段：`type=="array"` → Multi（选项取 `items.enum`/`items.oneOf`）；
  `type=="string"` 且含 `enum` → Single；`type=="string"` 无 `enum` → Text；其他类型 →
  整体 `Decline`（本期不支持的字段让 agent 自行处理）。空 properties → `Decline`。
- `build_elicitation_result`：single/text 取首值 → `string`；multi 取全部 → `string[]`；
  无答案 → `{"action":"decline"}`。`{"action":"accept","content":{...}}` 否则。

**单测**（纯函数）：single 映射（enum→options）、multi（items.enum）、text（无 enum）、
未知 type→Decline、空 properties→Decline、build 的 string/string[]/decline 三态。

### 3.2 请求处理（`acp.rs::handle_client_request`）

```rust
if method == "elicitation/create" {
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    match classify_elicitation(&params) {
        ElicitationPlan::Questions(qs) => {
            let request_id = conn.q_next.fetch_add(1, SeqCst).to_string();
            conn.pending_question.lock().unwrap()
                .insert(request_id.clone(), (id, session_id.clone(), qs.clone()));
            app.emit("acp://question", json!({
                "connId": conn.conn_id, "requestId": request_id,
                "sessionId": session_id, "questions": qs_ser,
            }));
        }
        ElicitationPlan::Decline => {
            // 直接回 decline（agent 自行判断），不 emit
            send_line(&json!({ "jsonrpc":"2.0", "id": id,
                "result": { "action": "decline" } }));
        }
    }
    return;
}
```
- `AcpConn` 增字段：`pending_question: Mutex<HashMap<String,(Value, Vec<Question>)>>`（+ `q_next: AtomicU64`）。
- `Question` 需 `Serialize`（camelCase：`id,prompt,kind,options[{value,label}]`）。

### 3.3 答复命令（`acp.rs` 新增 `#[tauri::command]`）

```rust
#[tauri::command]
pub fn acp_answer_elicitation(
    conn_id: String, request_id: String,
    answers: Vec<AnswerItem>,   // { fieldId, values: Vec<String> }
    declined: bool,
) -> Result<(), String>;
```
- 查 `pending_question.remove(request_id)` → 得 `(rpc_id, questions)`；
  构造 `build_elicitation_result` → `send_line` 回写 response；找不到 → `Ok(())`（安全 no-op）。
- 注册进 `lib.rs` 的 `invoke_handler`。

### 3.4 decline / cancel 路径

- `cancel_by_rpc_id`（`:389`）：扩展为同时扫 `pending_question`，命中则回
  `{"action":"decline"}`（协议要求收到方对原请求回响应）。
- 连接关闭/`acp_disconnect`：遍历 `pending_question` 回 decline（避免 agent hang）。
  （与现有 perm 的关闭清理同处。）

### 3.5 能力声明（仅 codex）

`initialize` params 构造处：当会话引擎 == codex 时，加
```json
"clientCapabilities": { "elicitation": { "form": {} } }
```
其他引擎不加（现状）。**判定按引擎类型，但只此一处**（PAC1：协议路由本身不含引擎名分支；
能力声明是 initialize 构造，属允许的「注册表/声明」范畴，与 spawn 构造同性质）。

## 4. 前端设计

### 4.1 类型与服务
- `src/types/acp.ts`：`AcpQuestion { id, prompt, kind: 'single'|'multi'|'text', options: {value,label}[] }`、
  `AcpQuestionEvent { connId, requestId, sessionId, questions }`。
- `src/services/acpService.ts`：`answerElicitation(connId, requestId, answers, declined)` 唯一
  invoke 点；isTauri 双通道 + mock（mock 立即 resolve）。

### 4.2 状态（`composables/useConversation.ts` 或新 store）
- 监听 `acp://question` → 写 `pendingQuestions: Map<connId, AcpQuestionEvent>`（本期一次一张，
  按 connId 覆盖/排队）。
- `answerQuestion(connId, requestId, answers, declined)` → 调 service + 本地标记已回答。

### 4.3 组件
- `src/views/coding/components/AskQuestionCard.vue`：
  - 渲染 `prompt` + 选项（single=radio / multi=checkbox）/ text=`<input>`。
  - 「提交」：收集答案 → `answerQuestion`；提交后按钮禁用并显示「已回答」。
  - 「跳过」：`declined=true`。
- 挂载：`CodingWorkspace.vue`（或会话视图）在 composer 上方渲染当前会话的 pending question
  （与权限卡同一区域/层级）。
- 回填：提交后向当前会话 push 一条本地 result 消息（问题 + 答案），格式对齐既有本地消息类型。

## 5. 兼容 / 回滚

- 回滚 = 删 `acp_question.rs` + `handle_client_request` 分支 + `acp_answer_elicitation` +
  前端组件/服务/事件监听；`pending_question` 字段可保留。
- **opencode 零回归**：opencode 不发 elicitation，且能力声明未变 → 行为逐字节不变。
- 非 form / 不支持类型 → decline（与当前 `-32601` 相比，agent 侧语义更友好：从「方法不存在」
  变「用户拒答」，codex 会据此自行判断而非报错）。

## 6. 测试要点

- Rust 单测（`acp_question.rs`）：classify 五态 + build 三态（见 §3.1）。
- 协议层：`handle_client_request` 对 `elicitation/create` 的 Questions→emit、
  Decline→直回，可在现有 classify 测试风格下补（不依赖真实连接的部分）。
- 前端：`pnpm vue-tsc` + `pnpm build`。
- 实机（人工）：codex Plan `request_user_input` 弹卡可答、agent 继续。

## 7. 风险

- **codex form 字段名漂移**（version-dependent）：用宽容解析 + 单测样例锁定；识别不了一律
  decline（不 panic、不 hang）。
- **一次多问**：本期一次一问（首字段）；若 codex 一次给多个问题，取第一个（其余忽略，注释说明）。
  多字段一次多问留后续。
- **`_meta.codex.isSecret`**：本期不处理（text 明文显示）——标注为已知限制。
