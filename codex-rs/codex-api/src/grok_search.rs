//! The supported Grok hosted-search policy, validated before HTTP transport.
//!
//! Wire restrictions: https://docs.x.ai/developers/tools/web-search and
//! https://docs.x.ai/developers/tools/x-search. Other restrictions fail closed.

use crate::error::ApiError;
use codex_protocol::grok::GrokXSearchOptions;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

pub(crate) fn project_web_search(
    fields: &Map<String, Value>,
    index: usize,
) -> Result<Value, ApiError> {
    let unsupported = || {
        ApiError::Stream(format!(
            "Grok cannot project web_search restrictions at tools[{index}]"
        ))
    };
    for (key, value) in fields {
        match key.as_str() {
            "type" | "filters" => {}
            // Cached, indexed, and live are stock mode hints. Grok hosted Web
            // Search has one wire mode; no equivalent restriction is claimed.
            "external_web_access" | "indexed_web_access" => {
                if !value.is_null() && !value.is_boolean() {
                    return Err(unsupported());
                }
            }
            "user_location" | "search_context_size" | "search_content_types" => {
                if !value.is_null() {
                    return Err(unsupported());
                }
            }
            _ => return Err(unsupported()),
        }
    }
    let mut projected = json!({"type": "web_search"});
    let Some(filters) = fields.get("filters").filter(|value| !value.is_null()) else {
        return Ok(projected);
    };
    let filters = filters.as_object().ok_or_else(unsupported)?;
    let mut restrictions = Map::new();
    for (key, value) in filters {
        if !matches!(key.as_str(), "allowed_domains" | "excluded_domains") {
            return Err(unsupported());
        }
        let domains = value.as_array().ok_or_else(unsupported)?;
        if domains.len() > 5
            || domains
                .iter()
                .any(|value| value.as_str().is_none_or(|domain| domain.trim().is_empty()))
        {
            return Err(ApiError::Stream(format!(
                "Grok tools[{index}].filters.{key} requires at most 5 nonempty domain strings"
            )));
        }
        if !domains.is_empty() {
            restrictions.insert(key.clone(), value.clone());
        }
    }
    if restrictions.len() > 1 {
        return Err(ApiError::Stream(format!(
            "Grok tools[{index}] cannot combine allowed_domains and excluded_domains"
        )));
    }
    if !restrictions.is_empty() {
        projected["filters"] = Value::Object(restrictions);
    }
    Ok(projected)
}

pub(crate) fn project_x_search(
    fields: &Map<String, Value>,
    defaults: Option<&GrokXSearchOptions>,
) -> Result<Value, ApiError> {
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "type" | "from_date" | "to_date"))
    {
        return Err(ApiError::Stream(
            "Grok cannot project unsupported x_search restrictions".into(),
        ));
    }
    let mut dates = defaults.cloned().unwrap_or_default();
    for (key, date) in [
        ("from_date", &mut dates.from_date),
        ("to_date", &mut dates.to_date),
    ] {
        if let Some(value) = fields.get(key).filter(|value| !value.is_null()) {
            *date = Some(
                value
                    .as_str()
                    .ok_or_else(|| {
                        ApiError::Stream(format!(
                            "Grok x_search.{key} must be a calendar YYYY-MM-DD"
                        ))
                    })?
                    .to_string(),
            );
        }
    }
    dates.validate().map_err(ApiError::Stream)?;
    let mut projected = json!({"type": "x_search"});
    if let Some(from) = dates.from_date {
        projected["from_date"] = json!(from);
    }
    if let Some(to) = dates.to_date {
        projected["to_date"] = json!(to);
    }
    Ok(projected)
}

#[cfg(test)]
#[path = "grok_search_tests.rs"]
mod tests;
