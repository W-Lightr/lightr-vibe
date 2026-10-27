// 自动化业务层（design §5.4 / §5.7）：纯逻辑，不感知 tokio、不发射事件。
//
// 所有函数只读写 `store::with_read` / `with_write`，由 engine 调用。
// 错误串沿用仓库词汇：not-found / io: ...

use super::cron;
use super::store::{
    self, now_ms, Automation, AutomationDraft, AutomationFile, AutomationRun,
    AutomationRunStatus, TriggerKind,
};

/// 运行记录保留天数（每 6 小时剪一次，与 codeg 同口径）
pub const RUN_RETENTION_DAYS: i64 = 30;

// ---------- 校验 ----------

/// 保存前的草稿校验。
///
/// 不合法**直接拒绝保存**：错误在编辑器里立刻可见，而不是变成一次失败的运行。
pub fn validate_draft(draft: &AutomationDraft) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        return Err("io: 自动化名称不能为空".to_string());
    }
    if draft.prompt.trim().is_empty() {
        return Err("io: 提示词不能为空".to_string());
    }
    if draft.project_id.trim().is_empty() {
        return Err("io: 必须选择一个目标项目".to_string());
    }
    if draft.root_path.trim().is_empty() {
        return Err("io: 目标项目缺少根目录".to_string());
    }
    if draft.engine_id.trim().is_empty() {
        return Err("io: 必须选择引擎".to_string());
    }
    if !matches!(draft.trigger_kind, TriggerKind::Manual) {
        let cron_expr = draft.cron.trim();
        if cron_expr.is_empty() {
            return Err("io: 定时触发必须填写 cron 表达式".to_string());
        }
        cron::validate_cron(cron_expr, &draft.timezone)?;
    }
    Ok(())
}

/// 下次触发时刻：仅「启用 + 定时」才计算，其余为 None。
pub fn next_run_for(draft: &AutomationDraft, now: i64) -> Result<Option<i64>, String> {
    if !draft.enabled || !matches!(draft.trigger_kind, TriggerKind::Schedule) {
        return Ok(None);
    }
    cron::next_run_for(draft.cron.trim(), &draft.timezone, now)
}

fn draft_into(draft: &AutomationDraft, next_run_at: Option<i64>, now: i64) -> Automation {
    Automation {
        id: String::new(),
        name: draft.name.trim().to_string(),
        enabled: draft.enabled,
        trigger_kind: draft.trigger_kind,
        cron: draft.cron.trim().to_string(),
        timezone: draft.timezone.trim().to_string(),
        next_run_at,
        engine_id: draft.engine_id.trim().to_string(),
        project_id: draft.project_id.trim().to_string(),
        root_path: draft.root_path.trim().to_string(),
        model_id: trim_to_none(draft.model_id.as_deref()),
        effort_id: trim_to_none(draft.effort_id.as_deref()),
        effort_value: trim_to_none(draft.effort_value.as_deref()),
        prompt: draft.prompt.clone(),
        action: draft.action,
        use_worktree: draft.use_worktree,
        base_branch: trim_to_none(draft.base_branch.as_deref()),
        init_command: trim_to_none(draft.init_command.as_deref()),
        last_run_at: None,
        last_run_status: None,
        last_run_id: None,
        unseen_failures: 0,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    }
}

fn trim_to_none(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn find_active(file: &AutomationFile, id: &str) -> Result<Automation, String> {
    file.automations
        .iter()
        .find(|a| a.id == id && a.deleted_at.is_none())
        .cloned()
        .ok_or_else(|| format!("not-found: automation {}", id))
}

// ---------- CRUD ----------

pub fn list() -> Result<Vec<Automation>, String> {
    store::with_read(|file| {
        let mut rows: Vec<Automation> = file
            .automations
            .iter()
            .filter(|a| a.deleted_at.is_none())
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        rows
    })
}

pub fn get(id: &str) -> Result<Automation, String> {
    store::with_read(|file| find_active(file, id))?
}

pub fn create(draft: AutomationDraft) -> Result<Automation, String> {
    validate_draft(&draft)?;
    let now = now_ms();
    let next = next_run_for(&draft, now)?;
    store::with_write(|file| {
        let mut row = draft_into(&draft, next, now);
        let mut id = store::create_id("auto");
        while file.automations.iter().any(|a| a.id == id) {
            id = store::create_id("auto");
        }
        row.id = id;
        file.automations.push(row.clone());
        Ok(row)
    })
}

pub fn update(id: &str, draft: AutomationDraft) -> Result<Automation, String> {
    validate_draft(&draft)?;
    let now = now_ms();
    let next = next_run_for(&draft, now)?;
    store::with_write(|file| {
        let idx = file
            .automations
            .iter()
            .position(|a| a.id == id && a.deleted_at.is_none())
            .ok_or_else(|| format!("not-found: automation {}", id))?;
        let old = file.automations[idx].clone();
        let mut row = draft_into(&draft, next, now);
        row.id = old.id;
        row.created_at = old.created_at;
        row.last_run_at = old.last_run_at;
        row.last_run_status = old.last_run_status;
        row.last_run_id = old.last_run_id;
        row.unseen_failures = old.unseen_failures;
        file.automations[idx] = row.clone();
        Ok(row)
    })
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<Automation, String> {
    let now = now_ms();
    store::with_write(|file| {
        let idx = file
            .automations
            .iter()
            .position(|a| a.id == id && a.deleted_at.is_none())
            .ok_or_else(|| format!("not-found: automation {}", id))?;
        let mut row = file.automations[idx].clone();
        row.enabled = enabled;
        row.updated_at = now;
        // 禁用清空下次触发；启用重算（时钟回拨也不会重复触发）
        row.next_run_at = if enabled && matches!(row.trigger_kind, TriggerKind::Schedule) {
            cron::next_run_for(&row.cron, &row.timezone, now)?
        } else {
            None
        };
        file.automations[idx] = row.clone();
        Ok(row)
    })
}

/// 软删：列表过滤，运行历史保留
pub fn soft_delete(id: &str) -> Result<(), String> {
    let now = now_ms();
    store::with_write(|file| {
        let idx = file
            .automations
            .iter()
            .position(|a| a.id == id && a.deleted_at.is_none())
            .ok_or_else(|| format!("not-found: automation {}", id))?;
        file.automations[idx].deleted_at = Some(now);
        file.automations[idx].next_run_at = None;
        file.automations[idx].updated_at = now;
        Ok(())
    })
}

/// 清零全部未读失败（进入自动化页时调用）
pub fn mark_all_seen() -> Result<(), String> {
    store::with_write(|file| {
        for a in file.automations.iter_mut() {
            a.unseen_failures = 0;
        }
        Ok(())
    })
}

// ---------- 调度原语 ----------

/// 到期的自动化 id（enabled + schedule + nextRunAt <= now + 未删）
pub fn list_due(now: i64) -> Result<Vec<String>, String> {
    store::with_read(|file| {
        file.automations
            .iter()
            .filter(|a| {
                a.deleted_at.is_none()
                    && a.enabled
                    && matches!(a.trigger_kind, TriggerKind::Schedule)
            })
            .filter(|a| a.next_run_at.map(|t| t <= now).unwrap_or(false))
            .map(|a| a.id.clone())
            .collect()
    })
}

/// CAS 抢占当前时间片：把 `nextRunAt` 从读到的旧值条件更新为 `now` 之后的下一时刻。
///
/// 抢到返回本次消费的时间片；被别人抢到 / 已不再到期返回 None。
/// `nextRunAt` 从 `now` 向前重算，所以停机跨越多个时点只补触发一次。
pub fn claim_due(id: &str, now: i64) -> Result<Option<i64>, String> {
    store::with_write(|file| {
        let Some(idx) = file.automations.iter().position(|a| a.id == id) else {
            return Ok(None);
        };
        let row = file.automations[idx].clone();
        if row.deleted_at.is_some()
            || !row.enabled
            || !matches!(row.trigger_kind, TriggerKind::Schedule)
        {
            return Ok(None);
        }
        let Some(slot) = row.next_run_at else {
            return Ok(None);
        };
        if slot > now {
            return Ok(None);
        }
        let next = cron::next_run_for(&row.cron, &row.timezone, now)?;
        file.automations[idx].next_run_at = next;
        Ok(Some(slot))
    })
}

// ---------- 运行原语 ----------

pub fn has_active_run(automation_id: &str) -> Result<bool, String> {
    store::with_read(|file| {
        file.runs
            .iter()
            .any(|r| r.automation_id == automation_id && !r.status.is_terminal())
    })
}

/// 插入一条 `running` 记录，并把父行的 lastRun* 标成 running
pub fn start_run(
    automation_id: &str,
    trigger: &str,
    scheduled_for: Option<i64>,
) -> Result<AutomationRun, String> {
    let now = now_ms();
    store::with_write(|file| {
        // 父行必须存在且未删，否则这次运行无处归属
        let idx = file
            .automations
            .iter()
            .position(|a| a.id == automation_id && a.deleted_at.is_none())
            .ok_or_else(|| format!("not-found: automation {}", automation_id))?;

        let mut run = AutomationRun {
            id: store::create_id("arun"),
            automation_id: automation_id.to_string(),
            status: AutomationRunStatus::Running,
            trigger: trigger.to_string(),
            scheduled_for,
            started_at: Some(now),
            ended_at: None,
            session_id: None,
            task_id: None,
            stop_reason: None,
            error: None,
            summary: None,
            created_at: now,
        };
        while file.runs.iter().any(|r| r.id == run.id) {
            run.id = store::create_id("arun");
        }
        file.runs.push(run.clone());

        let parent = &mut file.automations[idx];
        parent.last_run_at = Some(now);
        parent.last_run_status = Some(AutomationRunStatus::Running.as_str().to_string());
        parent.last_run_id = Some(run.id.clone());
        parent.updated_at = now;
        Ok(run)
    })
}

/// 记录一次「上一条还没跑完」被抑制的触发
pub fn record_skipped_run(
    automation_id: &str,
    trigger: &str,
    scheduled_for: Option<i64>,
) -> Result<AutomationRun, String> {
    let now = now_ms();
    store::with_write(|file| {
        if !file
            .automations
            .iter()
            .any(|a| a.id == automation_id && a.deleted_at.is_none())
        {
            return Err(format!("not-found: automation {}", automation_id));
        }
        let mut run = AutomationRun {
            id: store::create_id("arun"),
            automation_id: automation_id.to_string(),
            status: AutomationRunStatus::Skipped,
            trigger: trigger.to_string(),
            scheduled_for,
            started_at: Some(now),
            ended_at: Some(now),
            session_id: None,
            task_id: None,
            stop_reason: None,
            error: Some("previous run still active".to_string()),
            summary: None,
            created_at: now,
        };
        while file.runs.iter().any(|r| r.id == run.id) {
            run.id = store::create_id("arun");
        }
        file.runs.push(run.clone());
        Ok(run)
    })
}

/// 在运行还**没结束**时补写会话 id。
///
/// `launch_session` 拿到 `session/new` 就该立刻登记，而不是等 `settle_run`：
/// 否则运行历史在「运行中」期间读不到 sessionId，prd R7 的「用
/// acp_session_load 回放」也就无从谈起。
///
/// 只写 session_id，不动 status / ended_at —— 这不是结算。
pub fn attach_run_session(run_id: &str, session_id: &str) -> Result<(), String> {
    store::with_write(|file| {
        let Some(run) = file.runs.iter_mut().find(|r| r.id == run_id) else {
            return Err(format!("not-found: run {}", run_id));
        };
        // 已结算的不用再补：那时 session_id 已经在 settle 里写过了
        if run.status.is_terminal() {
            return Ok(());
        }
        run.session_id = Some(session_id.to_string());
        Ok(())
    })
}

/// 结算的附加事实（谁建的会话 / 任务、摘要、错误）/// 结算的附加事实（谁建的会话 / 任务、摘要、错误）
#[derive(Debug, Clone, Default)]
pub struct RunFacts {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub summary: Option<String>,
}

/// 结算一次运行。**幂等**：已是终态则返回 None（不重复结算、不重复计失败）。
///
/// 同时把结果写回父行的 lastRun*，失败时 `unseenFailures + 1`。
pub fn settle_run(
    run_id: &str,
    status: AutomationRunStatus,
    facts: RunFacts,
) -> Result<Option<AutomationRun>, String> {
    if !status.is_terminal() {
        return Err("io: settle_run 只能写入终态".to_string());
    }
    let now = now_ms();
    store::with_write(|file| {
        let Some(idx) = file.runs.iter().position(|r| r.id == run_id) else {
            return Err(format!("not-found: run {}", run_id));
        };
        if file.runs[idx].status.is_terminal() {
            return Ok(None);
        }
        let automation_id = file.runs[idx].automation_id.clone();
        {
            let run = &mut file.runs[idx];
            run.status = status;
            run.ended_at = Some(now);
            if let Some(v) = facts.session_id {
                run.session_id = Some(v);
            }
            if let Some(v) = facts.task_id {
                run.task_id = Some(v);
            }
            if let Some(v) = facts.stop_reason {
                run.stop_reason = Some(v);
            }
            if let Some(v) = facts.error {
                run.error = Some(v);
            }
            if let Some(v) = facts.summary {
                run.summary = Some(v);
            }
        }
        let settled = file.runs[idx].clone();

        if let Some(parent) = file.automations.iter_mut().find(|a| a.id == automation_id) {
            parent.last_run_at = Some(now);
            parent.last_run_status = Some(status.as_str().to_string());
            parent.last_run_id = Some(settled.id.clone());
            parent.updated_at = now;
            if matches!(status, AutomationRunStatus::Failed) {
                parent.unseen_failures = parent.unseen_failures.saturating_add(1);
            }
        }
        Ok(Some(settled))
    })
}

/// 运行历史（按创建时间倒序取最近 `limit` 条）
pub fn list_runs(automation_id: &str, limit: usize) -> Result<Vec<AutomationRun>, String> {
    let limit = limit.max(1);
    store::with_read(|file| {
        let mut rows: Vec<AutomationRun> = file
            .runs
            .iter()
            .filter(|r| r.automation_id == automation_id)
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        rows.truncate(limit);
        rows
    })
}

pub fn get_run(run_id: &str) -> Result<AutomationRun, String> {
    store::with_read(|file| {
        file.runs
            .iter()
            .find(|r| r.id == run_id)
            .cloned()
            .ok_or_else(|| format!("not-found: run {}", run_id))
    })?
}

/// 所有仍在 `running` 的记录
pub fn list_active_runs() -> Result<Vec<AutomationRun>, String> {
    store::with_read(|file| {
        file.runs
            .iter()
            .filter(|r| !r.status.is_terminal())
            .cloned()
            .collect()
    })
}

// ---------- 维护 ----------

/// 启动恢复：把所有 `running` 判死。
///
/// **只有在确认自己是唯一引擎时调用**（拿到独占文件锁之后），否则会把别的
/// 进程正在跑的运行误判死。绝不在这里补触发。
pub fn boot_reconcile_interrupted() -> Result<u64, String> {
    let active = list_active_runs()?;
    let mut n = 0u64;
    for run in active {
        if settle_run(
            &run.id,
            AutomationRunStatus::Failed,
            RunFacts {
                error: Some("interrupted by restart".to_string()),
                ..Default::default()
            },
        )?
        .is_some()
        {
            n += 1;
        }
    }
    Ok(n)
}

/// 删除超过 `keep_days` 的运行记录
pub fn prune_old_runs(keep_days: i64) -> Result<usize, String> {
    let cutoff = now_ms() - keep_days * 86_400_000;
    store::with_write(|file| {
        let before = file.runs.len();
        // 只剪已结算的；在飞的必须留着让对账看到
        file.runs
            .retain(|r| !r.status.is_terminal() || r.created_at >= cutoff);
        Ok(before - file.runs.len())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::store::{point_store_at, reset_store, test_guard};
    use std::path::PathBuf;

    fn temp_path(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lightr-automation-svc-{}-{}",
            tag,
            now_ms()
        ));
        let _ = std::fs::create_dir_all(&root);
        root.join("automations.json")
    }

    fn draft(name: &str, cron: &str) -> AutomationDraft {
        AutomationDraft {
            name: name.to_string(),
            enabled: true,
            trigger_kind: if cron.is_empty() {
                TriggerKind::Manual
            } else {
                TriggerKind::Schedule
            },
            cron: cron.to_string(),
            timezone: "UTC".to_string(),
            engine_id: "open_code".to_string(),
            project_id: "proj-a".to_string(),
            root_path: "/tmp/proj-a".to_string(),
            prompt: "检查依赖".to_string(),
            action: super::super::store::AutomationAction::EnqueueTask,
            use_worktree: true,
            ..Default::default()
        }
    }

    #[test]
    fn validate_rejects_missing_fields() {
        let mut d = draft("x", "0 9 * * *");
        d.name = "  ".into();
        assert!(validate_draft(&d).is_err());
        let mut d = draft("x", "0 9 * * *");
        d.prompt = "".into();
        assert!(validate_draft(&d).is_err());
        let mut d = draft("x", "0 9 * * *");
        d.cron = "bad cron".into();
        assert!(validate_draft(&d).is_err());
        let mut d = draft("x", "");
        d.trigger_kind = TriggerKind::Manual;
        assert!(validate_draft(&d).is_ok(), "手动触发不需要 cron");
    }

    #[test]
    fn create_computes_next_run_only_when_enabled_schedule() {
        let _g = test_guard();
        let path = temp_path("nextrun");
        point_store_at(&path);
        let a = create(draft("a", "0 9 * * *")).unwrap();
        assert!(a.next_run_at.is_some(), "启用 + 定时应算出下次触发");
        let m = create(draft("m", "")).unwrap();
        assert!(m.next_run_at.is_none(), "手动触发不应有 next_run_at");
        reset_store();
    }

    #[test]
    fn claim_due_is_exclusive_and_advances() {
        let _g = test_guard();
        let path = temp_path("claim");
        point_store_at(&path);
        let a = create(draft("min", "* * * * *")).unwrap();
        let now = now_ms();
        // 手工把 next_run_at 拨到过去
        store::with_write(|file| {
            let idx = file.automations.iter().position(|x| x.id == a.id).unwrap();
            file.automations[idx].next_run_at = Some(now - 300_000);
            Ok(())
        })
        .unwrap();

        assert_eq!(list_due(now).unwrap(), vec![a.id.clone()]);
        assert!(claim_due(&a.id, now).unwrap().is_some(), "第一次抢占应成功");
        assert!(claim_due(&a.id, now).unwrap().is_none(), "第二次抢占应失败");
        let after = get(&a.id).unwrap();
        assert!(after.next_run_at.unwrap() > now, "nextRunAt 应推进到未来");
        assert!(list_due(now).unwrap().is_empty(), "不再到期");
        reset_store();
    }

    #[test]
    fn disabled_automation_is_not_due() {
        let _g = test_guard();
        let path = temp_path("disabled");
        point_store_at(&path);
        let a = create(draft("x", "* * * * *")).unwrap();
        let now = now_ms();
        store::with_write(|file| {
            let idx = file.automations.iter().position(|x| x.id == a.id).unwrap();
            file.automations[idx].next_run_at = Some(now - 60_000);
            Ok(())
        })
        .unwrap();
        set_enabled(&a.id, false).unwrap();
        assert!(get(&a.id).unwrap().next_run_at.is_none(), "禁用应清空 nextRunAt");
        assert!(list_due(now).unwrap().is_empty());
        reset_store();
    }

    #[test]
    fn settle_is_idempotent_and_bumps_unseen_on_failure() {
        let _g = test_guard();
        let path = temp_path("settle");
        point_store_at(&path);
        let a = create(draft("x", "")).unwrap();
        let run = start_run(&a.id, "manual", None).unwrap();
        assert!(has_active_run(&a.id).unwrap());

        let first = settle_run(
            &run.id,
            AutomationRunStatus::Failed,
            RunFacts {
                error: Some("boom".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(first.is_some());
        // 第二次结算：幂等，不重复计失败
        let second = settle_run(&run.id, AutomationRunStatus::Succeeded, RunFacts::default()).unwrap();
        assert!(second.is_none());

        let parent = get(&a.id).unwrap();
        assert_eq!(parent.unseen_failures, 1);
        assert_eq!(parent.last_run_status.as_deref(), Some("failed"));
        assert!(!has_active_run(&a.id).unwrap());

        mark_all_seen().unwrap();
        assert_eq!(get(&a.id).unwrap().unseen_failures, 0);
        reset_store();
    }

    #[test]
    fn skipped_run_recorded() {
        let _g = test_guard();
        let path = temp_path("skip");
        point_store_at(&path);
        let a = create(draft("x", "")).unwrap();
        record_skipped_run(&a.id, "schedule", None).unwrap();
        let runs = list_runs(&a.id, 10).unwrap();
        assert_eq!(runs[0].status, AutomationRunStatus::Skipped);
        assert_eq!(runs[0].error.as_deref(), Some("previous run still active"));
        reset_store();
    }

    #[test]
    fn boot_reconcile_fails_every_running_run() {
        let _g = test_guard();
        let path = temp_path("boot");
        point_store_at(&path);
        let a = create(draft("x", "")).unwrap();
        start_run(&a.id, "schedule", None).unwrap();
        assert_eq!(boot_reconcile_interrupted().unwrap(), 1);
        assert!(!has_active_run(&a.id).unwrap());
        let runs = list_runs(&a.id, 10).unwrap();
        assert_eq!(runs[0].status, AutomationRunStatus::Failed);
        assert_eq!(runs[0].error.as_deref(), Some("interrupted by restart"));
        reset_store();
    }

    #[test]
    fn prune_removes_only_old_settled_runs() {
        let _g = test_guard();
        let path = temp_path("prune");
        point_store_at(&path);
        let a = create(draft("x", "")).unwrap();
        let old = start_run(&a.id, "manual", None).unwrap();
        settle_run(&old.id, AutomationRunStatus::Succeeded, RunFacts::default()).unwrap();
        let fresh = start_run(&a.id, "manual", None).unwrap();
        settle_run(&fresh.id, AutomationRunStatus::Succeeded, RunFacts::default()).unwrap();
        // 把 old 的 createdAt 拨到 60 天前
        store::with_write(|file| {
            let idx = file.runs.iter().position(|r| r.id == old.id).unwrap();
            file.runs[idx].created_at = now_ms() - 60 * 86_400_000;
            Ok(())
        })
        .unwrap();
        let removed = prune_old_runs(RUN_RETENTION_DAYS).unwrap();
        assert_eq!(removed, 1);
        let left = list_runs(&a.id, 10).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, fresh.id);
        reset_store();
    }

    /// 回归：`sessionId` 必须在运行**还没结束**时就能写进去。
    ///
    /// 早先只有 `settle_run` 会写 session_id，导致运行中读不到会话 id，
    /// prd R7 的「用 acp_session_load 回放」无从谈起。
    #[test]
    fn attach_run_session_writes_while_running() {
        let _g = test_guard();
        let path = temp_path("attach");
        point_store_at(&path);
        let a = create(draft("x", "")).unwrap();
        let run = start_run(&a.id, "manual", None).unwrap();

        attach_run_session(&run.id, "ses_abc123").unwrap();

        let got = get_run(&run.id).unwrap();
        assert_eq!(got.status, AutomationRunStatus::Running, "仍是运行中");
        assert_eq!(got.session_id.as_deref(), Some("ses_abc123"));
        reset_store();
    }

    /// 已结算的运行不再被 attach 覆盖（结算值才是最终事实）
    #[test]
    fn attach_run_session_is_noop_after_settle() {
        let _g = test_guard();
        let path = temp_path("attach2");
        point_store_at(&path);
        let a = create(draft("x", "")).unwrap();
        let run = start_run(&a.id, "manual", None).unwrap();
        settle_run(
            &run.id,
            AutomationRunStatus::Succeeded,
            RunFacts {
                session_id: Some("ses_final".into()),
                ..Default::default()
            },
        )
        .unwrap();

        attach_run_session(&run.id, "ses_late").unwrap();
        assert_eq!(
            get_run(&run.id).unwrap().session_id.as_deref(),
            Some("ses_final")
        );
        reset_store();
    }

    #[test]
    fn soft_deleted_automation_is_hidden_but_runs_survive() {
        let _g = test_guard();
        let path = temp_path("softdel");
        point_store_at(&path);
        let a = create(draft("x", "")).unwrap();
        let run = start_run(&a.id, "manual", None).unwrap();
        settle_run(&run.id, AutomationRunStatus::Succeeded, RunFacts::default()).unwrap();
        soft_delete(&a.id).unwrap();
        assert!(list().unwrap().is_empty());
        assert!(get(&a.id).is_err());
        assert_eq!(list_runs(&a.id, 10).unwrap().len(), 1, "运行历史应保留");
        reset_store();
    }
}
