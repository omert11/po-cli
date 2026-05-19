use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoEntry {
    pub msgid: String,
    pub msgstr: String,
    pub context: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PoStatistics {
    pub translated: usize,
    pub untranslated: usize,
    pub fuzzy: usize,
    pub obsolete: usize,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeOutput {
    pub file_path: String,
    pub statistics: PoStatistics,
    pub untranslated_entries: Vec<PoEntry>,
    pub fuzzy_entries: Vec<PoEntry>,
    pub obsolete_entries: Vec<PoEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationEntry {
    pub msgid: String,
    pub msgstr: String,
    pub context: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub msgid: String,
    pub msgstr: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidationOutput {
    pub invalids: Vec<ValidationResult>,
    pub total: usize,
    pub valid: bool,
}

#[derive(Debug, Serialize)]
pub struct UpdateResult {
    pub success: bool,
    pub updated_entries: usize,
    pub file_path: String,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidateAndUpdateOutput {
    pub validation: ValidationOutput,
    pub update: Option<UpdateResult>,
    pub message: String,
}
