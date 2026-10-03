use super::*;
use crate::{
    capacity::Kind, github::metadata::Lifecycle, monitoring::OperationState, panel::Detail,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    version: u8,
    sequence: u64,
    item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    pub limit: usize,
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub item_id: String,
    pub job: QueueJob,
    pub activity_sequence: u64,
    pub completed_passes: usize,
    pub review_attempts: usize,
    pub conversations: usize,
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub results: Vec<Row>,
    pub next_cursor: Option<Cursor>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    sequence: u64,
    rows: BTreeMap<String, Stamp>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    fingerprint: String,
    sequence: u64,
    row: Option<Row>,
}

fn add<T: Serialize>(
    parts: &mut BTreeMap<String, BTreeSet<String>>,
    id: String,
    value: &T,
) -> Result<(), String> {
    let bytes = serde_json::to_string(value).map_err(|_| "Cannot encode result activity.")?;
    parts.entry(id).or_default().insert(hash(&bytes));
    Ok(())
}

fn load_index(store: &Store) -> Result<Option<Index>, String> {
    match store.read_state("result-index.json") {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "Result index is invalid.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Cannot read result index.".into()),
    }
}

fn pending(store: &Store) -> Result<bool, String> {
    match store.read_state("result-index-pending.json") {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Result index recovery marker is invalid.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Cannot read result index recovery marker.".into()),
    }
}

pub(super) fn mark_pending(store: &Store) -> Result<(), String> {
    store.write_state("result-index-pending.json", &true)
}

pub(super) fn discard(store: &Store, items: &BTreeSet<String>) -> Result<(), String> {
    if let Some(mut index) = load_index(store)? {
        index.rows.retain(|id, _| !items.contains(id));
        store.write_state("result-index.json", &index)?;
    }
    Ok(())
}

pub(super) fn refresh(store: &Store) -> Result<(), String> {
    guard(store)?;
    let state = store.load_queue_state()?;
    let reviews = store.review_evidence()?;
    let publications = store.load_publications()?;
    let follows = store.load_follow_ups()?;
    let actions = store.load_actions()?;
    let feedback = store.load_feedback()?;
    let mut rows = BTreeMap::new();
    let mut parts = BTreeMap::new();
    let mut completed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for job in state.jobs.iter().chain(reviews.iter().map(|r| &r.job)) {
        let id = crate::queue::item_id(job);
        add(&mut parts, id.clone(), job)?;
        rows.entry(id.clone()).or_insert_with(|| Row {
            item_id: id,
            job: job.clone(),
            activity_sequence: 0,
            completed_passes: 0,
            review_attempts: 0,
            conversations: 0,
        });
    }
    for run in &reviews {
        let id = crate::queue::item_id(&run.job);
        add(&mut parts, id.clone(), run)?;
        if let Some(row) = rows.get_mut(&id) {
            row.review_attempts += 1;
            if run.operation.state == OperationState::Completed {
                completed
                    .entry(id.clone())
                    .or_default()
                    .insert(run.key.clone());
            }
        }
    }
    for p in publications {
        add(&mut parts, crate::queue::item_id(&p.review.job), &p)?;
    }
    for f in &follows {
        let id = crate::queue::item_id(&f.context.job);
        let intent = feedback
            .mentions
            .iter()
            .any(|m| m.key == f.key && m.work_id == f.id);
        // Execution linkage is bookkeeping, not another conversation or activity.
        if intent
            && f.phase == crate::follow_up::Phase::WaitingStart
            && f.analysis.is_none()
            && f.publication.is_none()
            && f.history.is_empty()
            && f.result.is_none()
            && f.error.is_none()
            && !f.cancelled
            && !f.uncertain
        {
            continue;
        }
        add(&mut parts, id.clone(), &f)?;
        if let Some(row) = rows.get_mut(&id).filter(|_| !intent) {
            row.conversations += 1;
        }
    }
    for mention in &feedback.mentions {
        let id = mention.item_id.clone().or_else(|| {
            follows
                .iter()
                .find(|f| f.key == mention.key && f.id == mention.work_id)
                .map(|f| crate::queue::item_id(&f.context.job))
        });
        if let Some(id) = id {
            add(
                &mut parts,
                id.clone(),
                &(&mention.key, &mention.work_id, &mention.comment),
            )?;
            if let Some(row) = rows.get_mut(&id) {
                row.conversations += 1;
            }
        }
    }
    for f in actions.finals {
        add(&mut parts, f.basis.item_id.clone(), &f)?;
    }
    for e in actions.effects {
        add(&mut parts, e.item_id.clone(), &e)?;
    }
    for o in actions.observations {
        // Successful unchanged provider reads are not new result activity.
        add(&mut parts, o.item_id.clone(), &(&o.observation, &o.error))?;
    }
    for r in feedback.records {
        add(&mut parts, crate::queue::item_id(&r.job), &r)?;
    }
    let mut index = load_index(store)?.unwrap_or_default();
    let fingerprints: BTreeMap<_, _> = parts
        .into_iter()
        .map(|(id, hashes)| (id, hash(&hashes.into_iter().collect::<Vec<_>>().join("\n"))))
        .collect();
    let changed = fingerprints.iter().any(|(id, fingerprint)| {
        index
            .rows
            .get(id)
            .is_none_or(|old| &old.fingerprint != fingerprint)
    });
    let removed = index.rows.keys().any(|id| !fingerprints.contains_key(id));
    if changed {
        index.sequence = index
            .sequence
            .checked_add(1)
            .ok_or("Result activity sequence exhausted.")?;
        for (id, fingerprint) in &fingerprints {
            if index
                .rows
                .get(id)
                .is_none_or(|old| &old.fingerprint != fingerprint)
            {
                index.rows.insert(
                    id.clone(),
                    Stamp {
                        fingerprint: fingerprint.clone(),
                        sequence: index.sequence,
                        row: None,
                    },
                );
            }
        }
    }
    index.rows.retain(|id, _| fingerprints.contains_key(id));
    let mut projection_changed = false;
    for (id, stamp) in &mut index.rows {
        let row =
            rows.remove(id).and_then(|mut row| {
                if state.tracked.iter().any(|p| {
                    Binding::tracked(p).matches(&row.job) && p.lifecycle != Lifecycle::Open
                }) {
                    return None;
                }
                row.activity_sequence = stamp.sequence;
                row.completed_passes = completed.get(id).map_or(0, BTreeSet::len);
                Some(row)
            });
        if stamp.row != row {
            stamp.row = row;
            projection_changed = true;
        }
    }
    if changed || removed || projection_changed {
        store.write_state("result-index.json", &index)?;
    }
    store.write_state("result-index-pending.json", &false)
}

pub fn page(store: &Store, request: PageRequest) -> Result<Page, String> {
    if !(1..=200).contains(&request.limit) {
        return Err(
            "Result page size must be between 1 and 200; there is no total result cap.".into(),
        );
    }
    if request
        .cursor
        .as_ref()
        .is_some_and(|c| c.version != 1 || c.item_id.is_empty() || c.item_id.len() > 16_384)
    {
        return Err("Result cursor is invalid; restart paging explicitly.".into());
    }
    guard(store)?;
    let mut index = load_index(store)?;
    if index.is_none() || pending(store)? {
        refresh(store)?;
        index = load_index(store)?;
    }
    let index = index.unwrap_or_default();
    if request
        .cursor
        .as_ref()
        .is_some_and(|c| c.sequence > index.sequence)
    {
        return Err("Result cursor belongs to an unavailable activity generation.".into());
    }
    let mut rows: Vec<_> = index.rows.into_values().filter_map(|s| s.row).collect();
    rows.sort_by(|a, b| {
        b.activity_sequence
            .cmp(&a.activity_sequence)
            .then(a.item_id.cmp(&b.item_id))
    });
    if let Some(cursor) = request.cursor {
        rows.retain(|row| {
            row.activity_sequence < cursor.sequence
                || row.activity_sequence == cursor.sequence && row.item_id > cursor.item_id
        });
    }
    let has_more = rows.len() > request.limit;
    rows.truncate(request.limit);
    let next_cursor = has_more
        .then(|| {
            rows.last().map(|r| Cursor {
                version: 1,
                sequence: r.activity_sequence,
                item_id: r.item_id.clone(),
            })
        })
        .flatten();
    Ok(Page {
        results: rows,
        next_cursor,
    })
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DetailResult {
    Available {
        destination: Detail,
        evidence: serde_json::Value,
    },
    Cleaned {
        destination: Detail,
        message: &'static str,
    },
    Missing {
        destination: Detail,
        message: &'static str,
    },
}

pub fn detail(store: &Store, destination: Detail) -> Result<DetailResult, String> {
    crate::panel::Route {
        tab: crate::panel::Tab::Reviewed,
        detail: Some(destination.clone()),
    }
    .validate()?;
    guard(store)?;
    if let Some(message) = cleaned(store, &destination)? {
        return Ok(DetailResult::Cleaned {
            destination,
            message,
        });
    }
    let jobs = store.load_queue()?;
    let reviews = store.review_evidence()?;
    let follows = store.load_follow_ups()?;
    let actions = store.load_actions()?;
    let evidence = match &destination {
        Detail::Item { item_id } => {
            let selected: Vec<_> = jobs
                .iter()
                .filter(|j| {
                    crate::queue::item_id(j) == *item_id
                        || j.work.as_ref().and_then(|w| w.legacy_item_id.as_ref()) == Some(item_id)
                })
                .collect();
            let canonical = selected.first().map(|j| crate::queue::item_id(j));
            let selected_reviews: Vec<_> = reviews
                .iter()
                .filter(|r| {
                    crate::queue::item_id(&r.job) == *item_id
                        || canonical.as_ref() == Some(&crate::queue::item_id(&r.job))
                })
                .collect();
            if selected.is_empty() && selected_reviews.is_empty() {
                None
            } else {
                let id = canonical.as_deref().unwrap_or(item_id);
                Some(serde_json::json!({
                    "jobs":selected,"reviews":selected_reviews,
                    "publications":store.load_publications()?.into_iter().filter(|p| crate::queue::item_id(&p.review.job)==id).collect::<Vec<_>>(),
                    "follow_ups":follows.iter().filter(|f| crate::queue::item_id(&f.context.job)==id).collect::<Vec<_>>(),
                    "finals":actions.finals.iter().filter(|f| f.basis.item_id==id).collect::<Vec<_>>(),
                    "effects":actions.effects.iter().filter(|e| e.item_id==id).collect::<Vec<_>>()
                }))
            }
        }
        Detail::Job {
            kind: Kind::Normal,
            id,
        } => {
            let attempts: Vec<_> = reviews.iter().filter(|r| r.key == *id).collect();
            let job = jobs.iter().find(|j| {
                j.assignment_id
                    .as_ref()
                    .is_some_and(|a| crate::review::key(j, a) == *id)
            });
            if job.is_none() && attempts.is_empty() {
                None
            } else {
                Some(serde_json::json!({"job":job,"attempts":attempts}))
            }
        }
        Detail::Job {
            kind: Kind::PrimaryFinal,
            id,
        } => actions
            .finals
            .iter()
            .find(|f| f.id == *id)
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| "Cannot encode final detail.")?,
        Detail::Job { kind, id } => {
            let run = follows
                .iter()
                .find(|f| f.id == *id && f.kind() == *kind)
                .map(serde_json::to_value)
                .transpose()
                .map_err(|_| "Cannot encode conversation detail.")?;
            if run.is_some() || *kind != Kind::Mention {
                run
            } else {
                store
                    .load_feedback()?
                    .mentions
                    .iter()
                    .find(|m| m.work_id == *id)
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|_| "Cannot encode mention detail.")?
            }
        }
        _ => return Err("Result detail requires an exact PR iteration or job.".into()),
    };
    Ok(match evidence {
        Some(evidence) => DetailResult::Available {
            destination,
            evidence,
        },
        None => DetailResult::Missing {
            destination,
            message:
                "This exact destination is missing. No other PR, iteration or job was selected.",
        },
    })
}
