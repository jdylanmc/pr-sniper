use super::{check_revision, revision, Error};
use crate::storage::{Agent, AiAccount, Assignment, Doctrine, Settings, Store};
use serde_json::{json, Value};

const SOLID: &str = "10300000-0000-4000-8000-000000000001";
const SECURITY: &str = "10300000-0000-4000-8000-000000000002";
const MAINTAINER: &str = "10300000-0000-4000-8000-000000000003";
const SECURITY_DOCTRINE: &str = "10300000-0000-4000-8000-000000000004";
const SOLID_DOCTRINE: &str = "10300000-0000-4000-8000-000000000005";

pub(crate) fn apply(
    store: &Store,
    expected: &str,
    account_id: &str,
    model: &str,
) -> Result<Value, Error> {
    let mut settings = store
        .load_settings()
        .map_err(|e| Error::new("storage", e))?;
    check_revision(expected, &settings)?;
    let original = settings.clone();
    let shipped_solid = crate::seeds::doctrines()
        .into_iter()
        .find(|d| d.title == "solid")
        .unwrap();
    let solid = doctrine(&mut settings, SOLID_DOCTRINE, "SOLID", &shipped_solid.body);
    let security = doctrine(
        &mut settings,
        SECURITY_DOCTRINE,
        "Security",
        include_str!("../../doctrines/security.md"),
    );
    for (id, name, doctrines, prompt) in [
        (SOLID, "SOLID", vec![solid], "Review the change using the selected SOLID doctrine. Submit a concise, evidence-backed teammate report to the Maintainer."),
        (SECURITY, "Security", vec![security], "Review the change for concrete exploitable security defects. Submit an evidence-backed teammate report to the Maintainer."),
        (MAINTAINER, "Maintainer", vec![], "Perform the final review after reading all teammate reports. Independently verify proposed findings against the change, reconcile disagreements, and choose an offered exit under the host's current gates. Do not rubber-stamp teammate conclusions."),
    ] {
        if settings.agents.iter().any(|a| a.id == id) { continue; }
        if settings.agents.iter().any(|a| a.name.eq_ignore_ascii_case(name)) {
            return Err(Error::new("preset_collision", json!({"component": name, "kind":"agent"})));
        }
        settings.agents.push(Agent {
            id: id.into(), name: name.into(), model: model.into(),
            intelligence: None, ai_account: Some(AiAccount { provider: "copilot".into(), account_id: account_id.into() }),
            doctrine: None, doctrines: None, doctrine_ids: doctrines, voice_id: None,
            prompt: prompt.into(), signature: name.into(),
        });
    }
    settings
        .validate()
        .map_err(|e| Error::new("validation", e))?;
    if settings != original {
        // One additive transaction. Existing components, defaults, repository
        // assignments, grants and account bindings are never overwritten.
        store
            .save_settings(&settings)
            .map_err(|e| Error::new("storage", e))?;
    }
    let assignments: Vec<_> = [SOLID, SECURITY, MAINTAINER]
        .iter()
        .map(|agent_id| Assignment {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: (*agent_id).into(),
            schedule: None,
            comment: false,
            approve: false,
            actions: None,
            tools: None,
            rubber_duck: None,
        })
        .collect();
    Ok(json!({
        "state":"saved", "revision": revision(&settings), "assignments": assignments,
        "primary_assignment_id": assignments[2].id,
        "agents": settings.agents.iter().filter(|a| [SOLID,SECURITY,MAINTAINER].contains(&a.id.as_str())).collect::<Vec<_>>()
    }))
}

fn doctrine(settings: &mut Settings, id: &str, title: &str, body: &str) -> String {
    if let Some(current) = settings
        .doctrines
        .iter()
        .find(|d| d.id == id || d.title.eq_ignore_ascii_case(title))
    {
        return current.id.clone();
    }
    settings.doctrines.push(Doctrine {
        id: id.into(),
        title: title.into(),
        body: body.into(),
    });
    id.into()
}
