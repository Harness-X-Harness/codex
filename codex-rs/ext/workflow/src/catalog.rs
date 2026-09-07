//! Named `/workflow` library. Persist JSON stays out of this scan.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use rhai::Dynamic;
use rhai::Engine;
use rhai::Map;

use crate::engine::MAX_WORKFLOW_OPERATIONS;
use crate::engine::MAX_WORKFLOW_SOURCE_BYTES;
use crate::engine::MAX_WORKFLOW_SOURCE_CHARS;
use crate::engine::WorkflowSourceError;
use crate::source_read::BoundedSourceError;
use crate::source_read::read_bounded_workflow_source;

const MAX_WORKFLOW_NAME_CHARS: usize = 64;
const MAX_WORKFLOW_DESCRIPTION_CHARS: usize = 512;
/// Inclusive cap on `.rhai` candidates inspected in one project or user scope.
pub const MAX_CATALOG_SCOPE_RHAI_FILES: usize = 256;

/// Library roots Host Goal reads. It does not read `~/.grok/**`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogRoots {
    pub user_dir: PathBuf,
    pub project_dir: PathBuf,
}

impl CatalogRoots {
    pub fn new(codex_home: impl Into<PathBuf>, project_root: impl Into<PathBuf>) -> Self {
        let codex_home = codex_home.into();
        let project_root = project_root.into();
        Self {
            user_dir: codex_home.join("workflows"),
            project_dir: project_root.join(".codex").join("workflows"),
        }
    }

    pub fn from_persist_and_cwd(persist_root: &Path, cwd: &Path) -> Self {
        let user_dir = persist_root.to_path_buf();
        let project_root = find_project_root(cwd);
        Self {
            user_dir,
            project_dir: project_root.join(".codex").join("workflows"),
        }
    }
}

/// A library script whose filename matches `meta.name`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogScript {
    pub name: String,
    pub description: String,
    pub source: String,
    pub path: PathBuf,
}

/// Why a catalog name could not be loaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogError {
    InvalidName(String),
    UnknownName(String),
    DuplicateName { name: String, scope: &'static str },
    FilenameMismatch { filename: String, name: String },
    Meta(String),
    SourceLimit(String),
    CatalogExceeds(String),
    Io { path: String, error: String },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(
                f,
                "invalid workflow name '{name}': expected lowercase letters, digits, or single hyphens"
            ),
            Self::UnknownName(name) => write!(f, "unknown workflow: {name}"),
            Self::DuplicateName { name, scope } => {
                write!(
                    f,
                    "ambiguous workflow '{name}': duplicate definitions in {scope} scope"
                )
            }
            Self::FilenameMismatch { filename, name } => {
                write!(
                    f,
                    "saved workflow filename '{filename}' must match meta.name '{name}'"
                )
            }
            Self::Meta(reason)
            | Self::SourceLimit(reason)
            | Self::CatalogExceeds(reason)
            | Self::Io { error: reason, .. } => f.write_str(reason),
        }
    }
}

impl std::error::Error for CatalogError {}

impl From<CatalogError> for WorkflowSourceError {
    fn from(error: CatalogError) -> Self {
        WorkflowSourceError::Invalid {
            reason: error.to_string(),
        }
    }
}

/// Load a named script from the project library, then the user library.
pub fn resolve_named(name: &str, roots: &CatalogRoots) -> Result<CatalogScript, CatalogError> {
    let name = normalize_catalog_name(name)?;
    let mut duplicates = BTreeMap::new();
    let project = scan_directory(&roots.project_dir, "project", &mut duplicates)?;
    if let Some(scope) = duplicates.get(&name) {
        return Err(CatalogError::DuplicateName { name, scope });
    }
    if let Some(entry) = project.into_iter().find(|entry| entry.name == name) {
        return Ok(entry);
    }
    if let Some(result) = load_regular_named_file(&roots.project_dir, &name) {
        return result;
    }
    let user = scan_directory(&roots.user_dir, "user", &mut duplicates)?;
    if let Some(scope) = duplicates.get(&name) {
        return Err(CatalogError::DuplicateName { name, scope });
    }
    if let Some(entry) = user.into_iter().find(|entry| entry.name == name) {
        return Ok(entry);
    }
    if let Some(result) = load_regular_named_file(&roots.user_dir, &name) {
        return result;
    }
    Err(CatalogError::UnknownName(name))
}

/// True when `token` is a catalog name, optionally with a `.rhai` suffix.
pub fn looks_like_catalog_name(token: &str) -> bool {
    normalize_catalog_name(token).is_ok()
}

pub fn normalize_catalog_name(token: &str) -> Result<String, CatalogError> {
    let trimmed = token.trim();
    let stem = trimmed.strip_suffix(".rhai").unwrap_or(trimmed).trim();
    if !is_valid_workflow_name(stem) {
        return Err(CatalogError::InvalidName(stem.to_string()));
    }
    Ok(stem.to_string())
}

fn scan_directory(
    dir: &Path,
    scope: &'static str,
    duplicates: &mut BTreeMap<String, &'static str>,
) -> Result<Vec<CatalogScript>, CatalogError> {
    let Ok(dir_meta) = std::fs::symlink_metadata(dir) else {
        return Ok(Vec::new());
    };
    if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
        return Ok(Vec::new());
    }
    let read_dir = std::fs::read_dir(dir).map_err(|error| CatalogError::Io {
        path: dir.display().to_string(),
        error: error.to_string(),
    })?;
    let mut paths = Vec::new();
    for entry in read_dir {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rhai") {
            continue;
        }
        if paths.len() >= MAX_CATALOG_SCOPE_RHAI_FILES {
            return Err(CatalogError::CatalogExceeds(format!(
                "workflow catalog exceeds {MAX_CATALOG_SCOPE_RHAI_FILES} files in {scope} scope"
            )));
        }
        paths.push(path);
    }
    paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    let mut entries = Vec::new();
    let mut seen = BTreeMap::new();
    for path in paths {
        match load_library_file(&path) {
            Ok(entry) => {
                *seen.entry(entry.name.clone()).or_insert(0usize) += 1;
                entries.push(entry);
            }
            Err(
                CatalogError::Io { .. }
                | CatalogError::Meta(_)
                | CatalogError::SourceLimit(_)
                | CatalogError::InvalidName(_)
                | CatalogError::FilenameMismatch { .. },
            ) => continue,
            Err(error) => return Err(error),
        }
    }
    for (name, count) in seen {
        if count > 1 {
            duplicates.insert(name.clone(), scope);
            entries.retain(|entry| entry.name != name);
        }
    }
    Ok(entries)
}

/// Reload a regular `{name}.rhai` after scan skipped it. Oversized regular
/// files keep scope ownership; symlink/non-regular/invalid scripts stay ignored.
fn load_regular_named_file(dir: &Path, name: &str) -> Option<Result<CatalogScript, CatalogError>> {
    let path = dir.join(format!("{name}.rhai"));
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return None;
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    match load_library_file(&path) {
        Ok(entry) => Some(Ok(entry)),
        Err(error @ CatalogError::SourceLimit(_)) => Some(Err(error)),
        Err(_) => None,
    }
}

fn load_library_file(path: &Path) -> Result<CatalogScript, CatalogError> {
    let source = match read_bounded_workflow_source(path) {
        Ok(source) => source,
        Err(BoundedSourceError::Io(error)) => {
            return Err(CatalogError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            });
        }
        Err(BoundedSourceError::TooManyBytes { actual }) => {
            return Err(CatalogError::SourceLimit(format!(
                "workflow source is {actual} bytes; max is {MAX_WORKFLOW_SOURCE_BYTES}"
            )));
        }
        Err(BoundedSourceError::NotUtf8) => {
            return Err(CatalogError::Io {
                path: path.display().to_string(),
                error: "workflow source must be UTF-8".to_string(),
            });
        }
        Err(BoundedSourceError::TooManyChars { actual }) => {
            return Err(CatalogError::SourceLimit(format!(
                "workflow source is {actual} characters; max is {MAX_WORKFLOW_SOURCE_CHARS}"
            )));
        }
    };
    let parsed = extract_meta(&source)?;
    validate_filename(path, &parsed.name)?;
    Ok(CatalogScript {
        name: parsed.name,
        description: parsed.description,
        source,
        path: path.to_path_buf(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowMeta {
    pub name: String,
    pub description: String,
}

pub fn extract_meta(source: &str) -> Result<WorkflowMeta, CatalogError> {
    if !first_statement_is_meta(source) {
        return Err(CatalogError::Meta(
            "first statement must be `let meta = #{ name, description };`".to_string(),
        ));
    }
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_WORKFLOW_OPERATIONS);
    engine.disable_symbol("eval");
    engine.disable_symbol("import");
    engine.compile(source).map_err(|error| {
        CatalogError::Meta(format!("workflow program is not valid Rhai: {error}"))
    })?;
    let mut scope = rhai::Scope::new();
    scope.push_dynamic("args", Dynamic::UNIT);
    let _ = engine.eval_with_scope::<Dynamic>(&mut scope, source);
    let meta = scope
        .get_value::<Map>("meta")
        .ok_or_else(|| CatalogError::Meta("meta is not a valid map".to_string()))?;
    let name = string_field(&meta, "name")?;
    let description = string_field(&meta, "description")?;
    if !is_valid_workflow_name(&name) {
        return Err(CatalogError::InvalidName(name));
    }
    if description.trim().is_empty() {
        return Err(CatalogError::Meta(
            "meta.description must be a non-empty string".to_string(),
        ));
    }
    if description.chars().count() > MAX_WORKFLOW_DESCRIPTION_CHARS {
        return Err(CatalogError::Meta(format!(
            "meta.description must be at most {MAX_WORKFLOW_DESCRIPTION_CHARS} characters"
        )));
    }
    Ok(WorkflowMeta { name, description })
}

fn string_field(meta: &Map, field: &str) -> Result<String, CatalogError> {
    let value = meta
        .get(field)
        .ok_or_else(|| CatalogError::Meta(format!("meta.{field} must be a non-empty string")))?;
    value
        .clone()
        .into_string()
        .map_err(|_| CatalogError::Meta(format!("meta.{field} must be a non-empty string")))
        .and_then(|text| {
            if text.trim().is_empty() {
                Err(CatalogError::Meta(format!(
                    "meta.{field} must be a non-empty string"
                )))
            } else if field == "name" && text.chars().count() > MAX_WORKFLOW_NAME_CHARS {
                Err(CatalogError::Meta(format!(
                    "meta.name must be at most {MAX_WORKFLOW_NAME_CHARS} characters"
                )))
            } else {
                Ok(text)
            }
        })
}

fn validate_filename(path: &Path, name: &str) -> Result<(), CatalogError> {
    let filename = path
        .file_name()
        .map(|filename| filename.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if path.extension().and_then(|ext| ext.to_str()) != Some("rhai")
        || !is_valid_workflow_name(stem)
    {
        return Err(CatalogError::InvalidName(filename));
    }
    if stem != name {
        return Err(CatalogError::FilenameMismatch {
            filename,
            name: name.to_string(),
        });
    }
    Ok(())
}

pub fn is_valid_workflow_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_WORKFLOW_NAME_CHARS
        && bytes
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes
            .last()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !bytes.windows(2).any(|pair| pair == b"--")
}

fn first_statement_is_meta(script: &str) -> bool {
    let mut rest = script;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("//") {
            rest = after.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
            continue;
        }
        if let Some(after) = rest.strip_prefix("/*") {
            match after.split_once("*/") {
                Some((_, next)) => {
                    rest = next;
                    continue;
                }
                None => return false,
            }
        }
        break;
    }
    rest.starts_with("let meta") || rest.starts_with("const meta")
}

fn find_project_root(cwd: &Path) -> PathBuf {
    let mut current = cwd.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return current;
        }
        if !current.pop() {
            return cwd.to_path_buf();
        }
    }
}

pub fn json_args_to_map(args: &serde_json::Map<String, serde_json::Value>) -> Map {
    let mut map = Map::new();
    for (key, value) in args {
        map.insert(key.clone().into(), json_to_dynamic(value));
    }
    map
}

fn json_to_dynamic(value: &serde_json::Value) -> Dynamic {
    match value {
        serde_json::Value::Null => Dynamic::UNIT,
        serde_json::Value::Bool(value) => Dynamic::from(*value),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(Dynamic::from)
            .or_else(|| value.as_f64().map(Dynamic::from))
            .unwrap_or_else(|| Dynamic::from(value.to_string())),
        serde_json::Value::String(value) => Dynamic::from(value.clone()),
        serde_json::Value::Array(values) => {
            Dynamic::from(values.iter().map(json_to_dynamic).collect::<rhai::Array>())
        }
        serde_json::Value::Object(values) => Dynamic::from(json_args_to_map(values)),
    }
}
