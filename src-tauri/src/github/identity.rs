use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    pub id: String,
    pub login: String,
}
