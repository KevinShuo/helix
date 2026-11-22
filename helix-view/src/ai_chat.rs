//! The most recent selection conversation belongs to the editor session. Its
//! source is frozen so a follow-up can be asked after editing or changing files.
#[derive(Debug)]
pub struct Session {
    pub label: String,
    pub source: String,
    pub language: String,
    pub turns: Vec<(String, String)>,
    pub pending: Option<String>,
    pub draft: String,
    pub notice: String,
    pub revision: u64,
    pub controller: helix_event::TaskController,
}
