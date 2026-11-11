//! 通用记忆模块：授权绑定当前两侧实例与 profile，不搬迁、不复制权威记录。
//!
//! 旧 use_approved 和逐条授权不代表双向全库许可；只有本文件的显式授权才启用。
use super::{
    query, share,
    store::{self, MemoryDoc, MemoryRecord, Namespace},
};
use crate::shell::{error::AppError, instance, paths, process, settings};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
struct Consent {
    version: u32,
    enabled: bool,
    pair_key: String,
}

#[derive(Serialize)]
pub(crate) struct Source {
    label: String,
    namespace: String,
}

#[derive(Serialize)]
pub(crate) struct Status {
    enabled: bool,
    pair_key: String,
    sources: Vec<Source>,
    warning: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Row {
    origin_label: String,
    source_namespace: String,
    record: MemoryRecord,
}

fn file() -> std::path::PathBuf {
    paths::shell_dir(settings::current_mode()).join("memory-sharing/module.json")
}

fn fail(reason: impl std::fmt::Display) -> AppError {
    AppError::Memory(format!(
        "通用记忆模块不可用：{reason}；请刷新记忆页面并检查 {}，原始记忆不会被修改。",
        file().display()
    ))
}

fn consent() -> Result<Consent, AppError> {
    match process::read_state_file::<Consent>(&file()) {
        process::StateRead::Missing => Ok(Consent {
            version: 1,
            ..Default::default()
        }),
        process::StateRead::Loaded(value) if value.version == 1 => Ok(value),
        process::StateRead::Loaded(_) => Err(fail("授权版本不受支持")),
        process::StateRead::Corrupt { reason } => Err(fail(reason)),
    }
}

pub(crate) fn pair() -> Result<Vec<Namespace>, AppError> {
    let dsh = Namespace::current();
    let mut sources = vec![dsh.clone()];
    if let Some(id) = crate::kernel::mcode_service::configured_instance_id().map_err(fail)? {
        sources.push(Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, &id));
    }
    if dsh.family != instance::KERNEL_FAMILY_DSH {
        return Err(fail("默认实例不是 DSH"));
    }
    let registry = instance::load_registry().map_err(fail)?;
    for ns in &sources {
        share::validate_coordinate(&ns.instance_id)?;
        share::validate_coordinate(&ns.profile)?;
        if !registry
            .get(&ns.instance_id)
            .is_some_and(|entry| entry.kernel_family == ns.family)
        {
            return Err(fail("实例注册信息已改变"));
        }
    }
    Ok(sources)
}

pub(crate) fn key(pair: &[Namespace]) -> String {
    pair.iter()
        .map(Namespace::key)
        .collect::<Vec<_>>()
        .join("|")
}

fn label(ns: &Namespace) -> &'static str {
    if ns.family == instance::KERNEL_FAMILY_DSH {
        "DSH"
    } else {
        "MiniMax"
    }
}

pub(crate) fn checked_doc(ns: &Namespace) -> Result<MemoryDoc, AppError> {
    let doc = store::load(ns)?;
    if doc.meta.schema_version > store::SCHEMA_VERSION {
        return Err(fail(format!("{} 记忆由更新版本写入", label(ns))));
    }
    Ok(doc)
}

pub(crate) fn status() -> Status {
    let result = (|| {
        let pair = pair()?;
        // 来源发现独立于旧互通授权文件。旧文件坏掉只关闭旧许可，
        // 不能让用户失去确认新公共池范围的入口，也不覆盖坏文件。
        let (enabled, warning) = match consent() {
            Ok(consent) => (
                consent.enabled && consent.pair_key == key(&pair),
                if consent.enabled && consent.pair_key != key(&pair) {
                    Some("实例或 profile 已改变，请重新确认当前公共池范围".into())
                } else {
                    None
                },
            ),
            Err(error) => (
                false,
                Some(format!(
                    "旧互通授权不可用，未沿用旧权限；可通过内核开关重新确认公共池：{error}"
                )),
            ),
        };
        Ok::<_, AppError>(Status {
            enabled,
            pair_key: key(&pair),
            sources: pair
                .iter()
                .map(|ns| Source {
                    label: label(ns).into(),
                    namespace: ns.key(),
                })
                .collect(),
            warning,
        })
    })();
    result.unwrap_or_else(|error| Status {
        enabled: false,
        pair_key: String::new(),
        sources: vec![],
        warning: Some(error.to_string()),
    })
}

pub(crate) fn set_enabled(enabled: bool, expected_pair: &str) -> Result<Status, AppError> {
    let _lock = share::lock_grants()?;
    // 撤销不要求原实例仍存在，坏授权也可由显式关闭恢复；不修改旧逐条许可。
    let pair_key = if enabled {
        let pair = pair()?;
        if key(&pair) != expected_pair {
            return Err(fail("实例或 profile 已改变，请重新确认"));
        }
        let _previous = consent()?;
        for ns in &pair {
            checked_doc(ns)?;
        }
        key(&pair)
    } else {
        String::new()
    };
    let bytes = serde_json::to_vec_pretty(&Consent {
        version: 1,
        enabled,
        pair_key,
    })
    .map_err(fail)?;
    crate::shell::atomic_write::atomic_write_private(&file(), &bytes).map_err(fail)?;
    Ok(status())
}

pub(crate) fn enabled_for(target: &Namespace) -> Result<bool, AppError> {
    // 新开关的授权与接收方绑定；一旦迁移，不再回退旧的双向或逐条授权。
    if let Some(enabled) = super::targets::pool_enabled_for(target)? {
        return Ok(enabled);
    }
    let consent = consent()?;
    if !consent.enabled {
        return Ok(false);
    }
    let pair = pair()?;
    Ok(consent.pair_key == key(&pair) && pair.contains(target))
}

/// 把插件回报的命中 id 解析成（来源命名空间, 库内 id）。
///
/// 模块开启后检索结果的 id 一律重写成 `<namespace>/<id>`（**本侧记录也加前缀**，
/// 见 [`retrieve`]），于是按裸 id 在当前库查永远是零命中。只认**当前授权的 pair**：
/// 桥上报的字符串来自模型，不能拿它当任意命名空间的路径。
/// 解析不出或不在授权范围时返回 `None`，调用方按旧行为（当前库裸 id）处理。
pub(crate) fn resolve_hit(current: &Namespace, id: &str) -> Option<(Namespace, String)> {
    let mut parts = id.splitn(4, '/');
    let (family, instance_id, profile, record_id) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if [family, instance_id, profile, record_id]
        .iter()
        .any(|part| part.is_empty())
    {
        return None;
    }
    // 上报方只能使用自己的授权。另一内核启用公共池不能替当前接收方授权。
    if super::targets::blocked(&current.family) || !enabled_for(current).ok()? {
        return None;
    }
    let pair = pair().ok()?;
    let source = Namespace {
        family: family.to_string(),
        instance_id: instance_id.to_string(),
        profile: profile.to_string(),
    };
    if !pair.contains(&source) {
        return None;
    }
    // 公共池只授权已审核的普通记忆，不让伪造前缀读取跨来源的交接/经验标题。
    let doc = checked_doc(&source).ok()?;
    if !doc.memories.iter().any(|record| {
        record.id == record_id
            && record.state == store::STATE_ACCEPTED
            && record.verification == store::VERIFICATION_VERIFIED
    }) {
        return None;
    }
    Some((source, record_id.to_string()))
}

/// 按命中 id 的**真实来源**记「被注入」计数。
///
/// 模块互通时命中 id 带命名空间前缀（本侧也是），直接交给当前库的 `note_recall`
/// 既找不到记录，也永远不会给来源库记账。
pub(crate) fn note_recall_for_hits(current: &Namespace, ids: &[&str]) -> Result<usize, AppError> {
    let mut groups: Vec<(Namespace, Vec<String>)> = Vec::new();
    for id in ids {
        let (source, record_id) =
            resolve_hit(current, id).unwrap_or_else(|| (current.clone(), (*id).to_string()));
        match groups.iter_mut().find(|(known, _)| *known == source) {
            Some((_, list)) => list.push(record_id),
            None => groups.push((source, vec![record_id])),
        }
    }
    let mut counted = 0;
    for (source, list) in groups {
        let refs: Vec<&str> = list.iter().map(String::as_str).collect();
        counted += store::note_recall(&source, &refs)?;
    }
    Ok(counted)
}

/// 「最近注入」卡片要显示的标题：模块互通时命中 id 是 `<namespace>/<id>`，
/// 只认已授权来源；可见性判据与 `record` 里按裸 id 查那一路相同（坐标只有路径）。
pub(crate) fn hit_title(current: &Namespace, id: &str, workspace: &str) -> Option<String> {
    let (source, record_id) = resolve_hit(current, id)?;
    let doc = store::load(&source).ok()?;
    let record = doc
        .memories
        .iter()
        .chain(doc.experiences.iter().map(|row| &row.record))
        .chain(doc.handoffs.iter().map(|row| &row.record))
        .find(|row| row.id == record_id)?;
    if record.title.trim().is_empty() || !query::scope_matches(record, "", workspace) {
        return None;
    }
    Some(record.title.clone())
}

pub(crate) fn list() -> Result<Vec<Row>, AppError> {
    let _lock = share::lock_grants()?;
    let pair = pair()?;
    let mut rows = Vec::new();
    for ns in &pair {
        rows.extend(checked_doc(ns)?.memories.into_iter().map(|record| Row {
            origin_label: label(ns).into(),
            source_namespace: ns.key(),
            record,
        }));
    }
    Ok(rows)
}

/// 两边桥的统一入口。未授权返回 None，由调用者保持原先的窄授权路径。
/// 调用者须持有共享锁直到响应写出，撤销不能与发送交错。
pub(crate) fn retrieve(
    target: &Namespace,
    text: &str,
    workspace_id: &str,
    workspace_path: &str,
) -> Result<Option<query::RetrievedContext>, AppError> {
    if !enabled_for(target)? {
        return Ok(if super::targets::pool_configured(&target.family)? {
            Some(query::RetrievedContext::default())
        } else {
            None
        });
    }
    let pair = pair()?;
    let workspace = super::workspaces::normalize_path(workspace_path);
    // 既有本侧许可下的交接/经验照旧；模块本身只新增普通记忆授权。
    let own = checked_doc(target)?;
    let mut combined = if own.meta.use_approved {
        own
    } else {
        MemoryDoc::default()
    };
    combined.memories.clear();
    combined.meta.generation = 0;
    for ns in &pair {
        let doc = checked_doc(ns)?;
        // 两库代次都影响插件缓存；不改变或写回原库的开关与计数。
        combined.meta.generation = combined.meta.generation.wrapping_add(doc.meta.generation);
        for mut record in doc.memories {
            // 历史自动接受但未审核的内容不因开启公共池而获得人工认可。
            if record.verification != store::VERIFICATION_VERIFIED {
                continue;
            }
            if record.scope != store::SCOPE_GENERAL && record.scope != store::SCOPE_PROJECT {
                continue;
            }
            let source_workspace_id = if ns == target { workspace_id } else { "" };
            if !query::scope_matches(&record, source_workspace_id, &workspace) {
                continue;
            }
            // 先按来源判断范围，再去掉内核局部 ID，避免跨内核 ID 碰撞。
            if ns != target {
                record.workspace_id.clear();
            }
            record.id = format!("{}/{}", ns.key(), record.id);
            combined.memories.push(record);
        }
    }
    // 模块/旧路径使用不同代次区间，避免另一侧空库时切换许可被插件视为“没有变化”。
    // 保持在 JavaScript 安全整数范围内，不用 u64 高位。
    combined.meta.generation = (combined.meta.generation & ((1u64 << 52) - 1)) | (1u64 << 52);
    Ok(Some(query::render_document_with_titles(
        &combined,
        text,
        workspace_id,
        &workspace,
        true,
        false,
        |record| {
            let source = pair
                .iter()
                .find(|ns| record.id.starts_with(&format!("{}/", ns.key())));
            match source {
                Some(ns) => format!("【来源：{}】{}", label(ns), record.title),
                None => record.title.clone(),
            }
        },
    )))
}

#[derive(Serialize)]
pub(crate) struct CandidateRow {
    origin_label: String,
    source_namespace: String,
    candidate: store::MemoryCandidate,
}

#[derive(Serialize)]
pub(crate) struct ReviewList {
    candidates: Vec<CandidateRow>,
    legacy_records: Vec<Row>,
}

fn source_namespace(source: &str) -> Result<Namespace, AppError> {
    pair()?
        .into_iter()
        .find(|ns| ns.key() == source)
        .ok_or_else(|| fail("来源空间已改变，请刷新审核列表"))
}

fn review_list() -> Result<ReviewList, AppError> {
    let _lock = share::lock_grants()?;
    let mut result = ReviewList {
        candidates: vec![],
        legacy_records: vec![],
    };
    for ns in pair()? {
        let doc = checked_doc(&ns)?;
        for candidate in doc.candidates {
            if candidate.state == store::CANDIDATE_PENDING {
                result.candidates.push(CandidateRow {
                    origin_label: label(&ns).into(),
                    source_namespace: ns.key(),
                    candidate,
                });
            }
        }
        for record in doc.memories {
            if record.state == store::STATE_ACCEPTED
                && record.verification != store::VERIFICATION_VERIFIED
            {
                result.legacy_records.push(Row {
                    origin_label: label(&ns).into(),
                    source_namespace: ns.key(),
                    record,
                });
            }
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn memory_pool_review_list() -> Result<ReviewList, String> {
    crate::commands::blocking(review_list).await
}

#[tauri::command]
pub async fn memory_pool_decide_candidate(
    source_namespace: String,
    id: String,
    accept: bool,
    scope: Option<String>,
) -> Result<Option<MemoryRecord>, String> {
    crate::commands::blocking(move || {
        let ns = self::source_namespace(&source_namespace)?;
        store::decide_candidate(&ns, &id, accept, scope.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn memory_pool_review_legacy(
    source_namespace: String,
    id: String,
    expected_revision: u64,
    accept: bool,
) -> Result<MemoryRecord, String> {
    crate::commands::blocking(move || {
        let ns = self::source_namespace(&source_namespace)?;
        store::review_legacy_record(&ns, &id, expected_revision, accept)
    })
    .await
}

#[tauri::command]
pub async fn memory_module_status() -> Result<Status, String> {
    crate::commands::blocking(|| Ok::<_, String>(status())).await
}

#[tauri::command]
pub async fn memory_module_set_enabled(
    enabled: bool,
    expected_pair: String,
) -> Result<Status, String> {
    crate::commands::blocking(move || set_enabled(enabled, &expected_pair)).await
}

#[tauri::command]
pub async fn memory_module_list() -> Result<Vec<Row>, String> {
    crate::commands::blocking(list).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::testkit::{general_input, TempHome};

    fn fixture(tag: &str) -> (TempHome, [Namespace; 2]) {
        let home = TempHome::new(tag);
        let dsh = Namespace::current();
        let mcode = Namespace::for_bridge(instance::KERNEL_FAMILY_MCODE, "module-mini");
        let mut registry = instance::InstanceRegistry::default();
        for (ns, port) in [(&dsh, 31001), (&mcode, 31002)] {
            let entry = instance::InstanceRecord::new(&ns.instance_id, &ns.family, port, 1);
            instance::save_record_to_disk(&entry).unwrap();
            registry.add(entry).unwrap();
        }
        instance::save_registry(&registry).unwrap();
        let path = paths::shell_dir(settings::current_mode()).join("mcode.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            r#"{"instance_id":"module-mini","port":31002,"miniapp_port":31003}"#,
        )
        .unwrap();
        (home, [dsh, mcode])
    }

    fn seed(ns: &Namespace, text: &str) -> MemoryRecord {
        let mut input = general_input(text, text);
        input.pinned = true;
        store::save(ns, input).unwrap()
    }

    #[test]
    fn explicit_module_consent_is_bidirectional_and_preserves_legacy_bytes() {
        let (_home, pair) = fixture("module-bidirectional");
        let first = seed(&pair[0], "来自 DSH 的偏好");
        seed(&pair[1], "来自 MiniMax 的偏好");
        // 合成两库相同 ID，注入身份仍必须按来源区分。
        let mut peer = checked_doc(&pair[1]).unwrap();
        peer.memories[0].id = first.id;
        std::fs::write(pair[1].file(), serde_json::to_vec(&peer).unwrap()).unwrap();
        store::set_use_approved(&pair[0], true).unwrap();
        let before: Vec<_> = pair
            .iter()
            .map(|ns| std::fs::read(ns.file()).unwrap())
            .collect();
        assert!(
            retrieve(&pair[0], "", "", "").unwrap().is_none(),
            "旧单侧许可不能升级为共享"
        );
        assert!(set_enabled(true, "过期的配对").is_err());
        set_enabled(true, &key(&pair)).unwrap();
        for target in &pair {
            let context = retrieve(target, "", "", "").unwrap().unwrap();
            assert!(context.enabled);
            assert!(context.generation >= 1u64 << 52);
            assert_eq!(context.hits.len(), 2);
            assert_ne!(context.hits[0].id, context.hits[1].id);
            assert!(context.text.contains("来源：DSH"));
            assert!(context.text.contains("来源：MiniMax"));
            assert!(!context.text.contains("memory_read"));
        }
        assert_eq!(list().unwrap().len(), 2);
        set_enabled(false, "").unwrap();
        assert!(retrieve(&pair[1], "", "", "").unwrap().is_none());
        for (ns, bytes) in pair.iter().zip(before) {
            assert_eq!(std::fs::read(ns.file()).unwrap(), bytes);
        }
    }

    #[test]
    fn source_labels_do_not_change_title_recall() {
        let (_home, pair) = fixture("module-title-recall");
        for ns in &pair {
            let mut input = general_input("构建前先跑检查", "独立正文，不与查询重合");
            input.keywords.clear();
            input.pinned = false;
            store::save(ns, input).unwrap();
        }
        set_enabled(true, &key(&pair)).unwrap();
        for target in &pair {
            for query in ["构建前先跑检查", "请确认构建前先跑检查这条约定还有效吗"]
            {
                let context = retrieve(target, query, "", "").unwrap().unwrap();
                assert_eq!(context.hits.len(), 2, "{query}");
                assert!(context.text.contains("【来源：DSH】构建前先跑检查"));
                assert!(context.text.contains("【来源：MiniMax】构建前先跑检查"));
                assert_eq!(context.total_bytes, context.text.len());
                assert!(context.total_bytes <= query::CONTEXT_TOTAL_BYTES);
            }
            // 展示标记不是可搜索内容，不应凭「来源」二字制造命中。
            assert!(retrieve(target, "来源", "", "")
                .unwrap()
                .unwrap()
                .hits
                .is_empty());
        }
    }

    #[test]
    fn source_labels_are_included_in_context_budgets() {
        let (_home, pair) = fixture("module-source-budget");
        for i in 0..8 {
            let ns = &pair[i % pair.len()];
            let body = format!("{i}{}", "字".repeat((store::MAX_BODY_BYTES - 1) / 3));
            let mut input = general_input(&format!("长记忆 {i}"), &body);
            input.pinned = true;
            store::save(ns, input).unwrap();
        }
        set_enabled(true, &key(&pair)).unwrap();
        let context = retrieve(&pair[0], "", "", "").unwrap().unwrap();
        assert!(context.hits.iter().any(|hit| hit.truncated));
        assert!(context.text.contains("因体积上限未展开"));
        assert!(context.text.contains("因条数上限"));
        assert!(context
            .hits
            .iter()
            .all(|hit| hit.title.starts_with("【来源：")));
        assert_eq!(context.total_bytes, context.text.len());
        assert!(context.total_bytes <= query::CONTEXT_TOTAL_BYTES);
    }

    #[test]
    fn project_scope_disabled_records_and_namespace_changes_fail_closed() {
        let (_home, pair) = fixture("module-scope");
        let workspace = std::env::temp_dir()
            .join("module-project-a")
            .to_string_lossy()
            .into_owned();
        let mut input = general_input("项目约束", "项目私有正文");
        input.scope = store::SCOPE_PROJECT.into();
        input.workspace_path = workspace.clone();
        input.pinned = true;
        let project = store::save(&pair[0], input).unwrap();
        let disabled = seed(&pair[1], "不应注入");
        store::set_state(&pair[1], &disabled.id, store::STATE_DISABLED).unwrap();
        set_enabled(true, &key(&pair)).unwrap();
        assert!(retrieve(&pair[1], "", "", "")
            .unwrap()
            .unwrap()
            .hits
            .is_empty());
        assert!(retrieve(&pair[1], "", "", "/other-project")
            .unwrap()
            .unwrap()
            .hits
            .is_empty());
        assert_eq!(
            retrieve(&pair[1], "", "", &workspace)
                .unwrap()
                .unwrap()
                .hits
                .len(),
            1
        );
        store::delete(&pair[0], &project.id).unwrap();
        assert!(retrieve(&pair[1], "", "", &workspace)
            .unwrap()
            .unwrap()
            .hits
            .is_empty());
        let mut wrong_profile = pair[1].clone();
        wrong_profile.profile = "other".into();
        assert!(!enabled_for(&wrong_profile).unwrap());
        let mut wrong_instance = pair[1].clone();
        wrong_instance.instance_id = "another-instance".into();
        assert!(!enabled_for(&wrong_instance).unwrap());
        std::fs::write(
            file(),
            r#"{"version":2,"enabled":true,"pair_key":"future"}"#,
        )
        .unwrap();
        assert!(retrieve(&pair[1], "", "", "").is_err());
        set_enabled(false, "").unwrap();
        assert!(!status().enabled);
    }

    /// 模块互通会把命中 id 重写成 `namespace/id`（本侧也加前缀）：注入计数要记到
    /// **来源库**、标题要能按来源解析，而不是拿全限定 id 在当前库查成零命中。
    #[test]
    fn qualified_hits_are_counted_and_titled_in_their_source_library() {
        let (_home, pair) = fixture("module-qualified-hits");
        let dsh = seed(&pair[0], "DSH 的记录");
        let mcode = seed(&pair[1], "MiniMax 的记录");
        set_enabled(true, &key(&pair)).unwrap();
        // 插件上报的就是检索真正返回的 hit id（不是这里自己拼的形状）。
        let context = retrieve(&pair[0], "记录", "", "").unwrap().unwrap();
        let ids: Vec<String> = context.hits.iter().map(|hit| hit.id.clone()).collect();
        assert_eq!(ids.len(), 2, "两库都要命中：{:?}", ids);
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        assert_eq!(
            note_recall_for_hits(&pair[0], &refs).unwrap(),
            2,
            "两条命中都要记到各自来源"
        );
        for (ns, before) in [(&pair[0], &dsh), (&pair[1], &mcode)] {
            let after = store::find_by_id(ns, &before.id, true)
                .unwrap()
                .expect("record");
            assert_eq!(after.recall_count, 1, "{} 的来源记录各 +1", ns.key());
        }
        let mut titles: Vec<String> = ids
            .iter()
            .filter_map(|id| hit_title(&pair[0], id, ""))
            .collect();
        titles.sort();
        assert_eq!(
            titles,
            vec!["DSH 的记录", "MiniMax 的记录"],
            "两侧来源的标题都要能解析"
        );

        // 撤销授权后不再按前缀分账：桥上报的 id 来自模型，不能当任意命名空间的路径。
        set_enabled(false, "").unwrap();
        assert_eq!(note_recall_for_hits(&pair[0], &refs).unwrap(), 0);
        assert!(hit_title(&pair[0], &ids[0], "").is_none());
    }

    #[test]
    fn corrupt_source_does_not_get_overwritten_or_silently_shared() {
        let (_home, pair) = fixture("module-corrupt-source");
        seed(&pair[0], "偏好");
        seed(&pair[1], "偏好");
        set_enabled(true, &key(&pair)).unwrap();
        std::fs::write(pair[1].file(), "broken").unwrap();
        assert!(retrieve(&pair[0], "", "", "").is_err());
        assert!(list().is_err());
        set_enabled(false, "").unwrap();
        assert_eq!(std::fs::read_to_string(pair[1].file()).unwrap(), "broken");
    }
    #[test]
    fn pool_target_is_single_atomic_consent_and_never_changes_the_other_target() {
        let (_home, pair) = fixture("pool-target");
        seed(&pair[0], "DSH 偏好");
        seed(&pair[1], "MiniMax 偏好");
        assert!(super::super::targets::set_pool("dsh", true, "stale").is_err());
        super::super::targets::set_pool("dsh", true, &key(&pair)).unwrap();
        assert!(enabled_for(&pair[0]).unwrap());
        assert!(
            !enabled_for(&pair[1]).unwrap(),
            "开启 DSH 不能顺带开启 MiniMax"
        );
        assert_eq!(
            retrieve(&pair[0], "", "", "").unwrap().unwrap().hits.len(),
            2
        );
        super::super::targets::set_pool("mcode", true, &key(&pair)).unwrap();
        assert_eq!(
            retrieve(&pair[1], "", "", "").unwrap().unwrap().hits.len(),
            2
        );
        // 即使旧授权还在，关闭新开关也不能回退旧权限。
        set_enabled(true, &key(&pair)).unwrap();
        store::set_use_approved(&pair[0], true).unwrap();
        let closed = super::super::targets::set_pool("dsh", false, "").unwrap();
        let view = serde_json::to_value(closed).unwrap();
        assert_eq!(view[0]["pool_enabled"], false);
        assert_eq!(
            view[0]["needs_consent"], false,
            "关闭不遗忘已经确认的来源范围"
        );
        assert!(!enabled_for(&pair[0]).unwrap());
        assert!(super::super::targets::blocked(&pair[0].family));
        assert!(!retrieve(&pair[0], "", "", "").unwrap().unwrap().enabled);
        assert!(enabled_for(&pair[1]).unwrap());
        // 兼容旧命令的关闭也必须真正在运行时阻断。
        super::super::targets::set("mcode", false).unwrap();
        assert!(!enabled_for(&pair[1]).unwrap());
        assert!(super::super::targets::blocked(&pair[1].family));
    }

    #[test]
    fn single_kernel_pool_and_source_change_require_new_consent() {
        let (_home, pair) = fixture("pool-single");
        let mcode_config = paths::shell_dir(settings::current_mode()).join("mcode.json");
        let saved = std::fs::read(&mcode_config).unwrap();
        std::fs::remove_file(&mcode_config).unwrap();
        let current = super::pair().unwrap();
        assert_eq!(current.len(), 1);
        seed(&current[0], "单内核偏好");
        super::super::targets::set_pool("dsh", true, &key(&current)).unwrap();
        assert_eq!(
            retrieve(&current[0], "", "", "")
                .unwrap()
                .unwrap()
                .hits
                .len(),
            1
        );
        assert!(super::super::targets::set_pool("mcode", true, &key(&current)).is_err());
        std::fs::write(mcode_config, saved).unwrap();
        assert!(!enabled_for(&pair[0]).unwrap());
        assert!(super::super::targets::blocked(&pair[0].family));
        assert!(!retrieve(&pair[0], "", "", "").unwrap().unwrap().enabled);
        super::super::targets::set_pool("dsh", true, &key(&pair)).unwrap();
        assert!(enabled_for(&pair[0]).unwrap());
    }

    #[test]
    fn legacy_unverified_records_wait_for_review_before_entering_pool() {
        let (_home, pair) = fixture("pool-legacy-review");
        let mut input = general_input("历史自动记忆", "未审核正文");
        input.verification = store::VERIFICATION_UNVERIFIED.into();
        input.pinned = true;
        let record = store::save(&pair[1], input).unwrap();
        super::super::targets::set_pool("dsh", true, &key(&pair)).unwrap();
        assert!(retrieve(&pair[0], "", "", "")
            .unwrap()
            .unwrap()
            .hits
            .is_empty());
        let review = review_list().unwrap();
        assert_eq!(review.legacy_records.len(), 1);
        assert_eq!(review.legacy_records[0].source_namespace, pair[1].key());
        store::review_legacy_record(&pair[1], &record.id, record.revision, true).unwrap();
        assert_eq!(
            retrieve(&pair[0], "", "", "").unwrap().unwrap().hits.len(),
            1
        );
        assert!(review_list().unwrap().legacy_records.is_empty());
        assert!(source_namespace("mcode:untrusted/default").is_err());
    }
    #[test]
    fn corrupt_legacy_consent_does_not_block_new_pool_controls() {
        let (_home, pair) = fixture("pool-corrupt-legacy");
        seed(&pair[0], "已审核偏好");
        std::fs::create_dir_all(file().parent().unwrap()).unwrap();
        std::fs::write(file(), "broken legacy consent").unwrap();
        let before = status();
        assert!(!before.enabled, "旧坏授权不能被视为有效");
        assert!(before.warning.is_some());
        assert_eq!(before.pair_key, key(&pair));
        assert_eq!(before.sources.len(), 2);
        super::super::targets::set_pool("dsh", true, &before.pair_key).unwrap();
        assert!(enabled_for(&pair[0]).unwrap());
        assert_eq!(
            retrieve(&pair[0], "", "", "").unwrap().unwrap().hits.len(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(file()).unwrap(),
            "broken legacy consent"
        );
        super::super::targets::set_pool("dsh", false, "").unwrap();
        assert!(!enabled_for(&pair[0]).unwrap());
        assert!(!status().pair_key.is_empty());
    }
    #[test]
    fn legacy_sharing_does_not_approve_new_pool_extraction() {
        let (_home, pair) = fixture("pool-legacy-consent");
        set_enabled(true, &key(&pair)).unwrap();
        assert!(!checked_doc(&pair[0]).unwrap().meta.use_approved);
        let legacy = serde_json::to_value(super::super::targets::status()).unwrap();
        assert_eq!(legacy[0]["pool_enabled"], true);
        assert_eq!(
            legacy[0]["needs_consent"], true,
            "旧双向引用许可不包含新模式的自动提取授权"
        );
        assert!(!super::super::targets::extraction_enabled(&pair[0]).unwrap());
        super::super::targets::set_pool("dsh", true, &key(&pair)).unwrap();
        let confirmed = serde_json::to_value(super::super::targets::status()).unwrap();
        assert_eq!(confirmed[0]["needs_consent"], false);
        assert!(super::super::targets::extraction_enabled(&pair[0]).unwrap());
    }
    #[test]
    fn new_pool_only_hits_are_attributed_only_for_the_authorized_receiver() {
        let (_home, pair) = fixture("pool-qualified-hits");
        let dsh = seed(&pair[0], "DSH 审核记录");
        let mini = seed(&pair[1], "MiniMax 审核记录");
        // 不写旧 module.json，只启用 DSH 的新公共池开关。
        super::super::targets::set_pool("dsh", true, &key(&pair)).unwrap();
        assert!(!file().exists());
        let context = retrieve(&pair[0], "", "", "").unwrap().unwrap();
        let ids: Vec<String> = context.hits.iter().map(|hit| hit.id.clone()).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(note_recall_for_hits(&pair[0], &refs).unwrap(), 2);
        for (ns, record) in [(&pair[0], dsh), (&pair[1], mini)] {
            assert_eq!(
                store::find_by_id(ns, &record.id, true)
                    .unwrap()
                    .unwrap()
                    .recall_count,
                1
            );
        }
        assert_eq!(
            ids.iter()
                .filter_map(|id| hit_title(&pair[0], id, ""))
                .count(),
            2
        );
        assert_eq!(
            note_recall_for_hits(&pair[1], &refs).unwrap(),
            0,
            "未启用的接收方不能借另一侧权限分账"
        );
        assert!(ids.iter().all(|id| hit_title(&pair[1], id, "").is_none()));
        super::super::targets::set_pool("dsh", false, "").unwrap();
        assert_eq!(note_recall_for_hits(&pair[0], &refs).unwrap(), 0);
        assert!(ids.iter().all(|id| hit_title(&pair[0], id, "").is_none()));
    }
}
