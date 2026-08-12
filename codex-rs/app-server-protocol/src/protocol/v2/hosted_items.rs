use crate::JsonSchema;
use crate::TS;
use codex_extension_items::image_generation::ImageGenerationItem as ExtensionImageGenerationItem;
pub use codex_extension_items::web_search::SearchSource;
pub use codex_extension_items::web_search::WebSearchAction;
use codex_extension_items::web_search::WebSearchItem as ExtensionWebSearchItem;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;

/// App-server projection of a hosted or standalone web search.
///
/// This type is separate from the durable extension item so v2 can keep its
/// required-nullable field contract without changing rollout serialization.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub struct WebSearchItem {
    pub id: String,
    pub query: String,
    pub action: Option<WebSearchAction>,
    pub results: Option<Vec<JsonValue>>,
    pub source: Option<SearchSource>,
}

impl From<ExtensionWebSearchItem> for WebSearchItem {
    fn from(item: ExtensionWebSearchItem) -> Self {
        Self {
            id: item.id,
            query: item.query,
            action: item.action,
            results: item.results,
            source: item.source,
        }
    }
}

/// App-server projection of an image-generation result.
///
/// This type is separate from the durable extension item so v2 can keep its
/// required-nullable field contract without changing rollout serialization.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase", export_to = "v2/")]
pub struct ImageGenerationItem {
    pub id: String,
    pub status: String,
    pub revised_prompt: Option<String>,
    pub prompt: Option<String>,
    pub result: String,
    pub transparent_background: Option<bool>,
    pub saved_path: Option<AbsolutePathBuf>,
}

impl From<ExtensionImageGenerationItem> for ImageGenerationItem {
    fn from(item: ExtensionImageGenerationItem) -> Self {
        Self {
            id: item.id,
            status: item.status,
            revised_prompt: item.revised_prompt,
            prompt: item.prompt,
            result: item.result,
            transparent_background: item.transparent_background,
            saved_path: item.saved_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::v2::ThreadItem;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn hosted_item_v2_fields_are_required_and_nullable() {
        let web_search = ThreadItem::WebSearch(WebSearchItem {
            id: "search-1".to_string(),
            query: "docs".to_string(),
            action: None,
            results: None,
            source: None,
        });
        let image_generation = ThreadItem::ImageGeneration(ImageGenerationItem {
            id: "image-1".to_string(),
            status: "completed".to_string(),
            revised_prompt: None,
            prompt: None,
            result: String::new(),
            transparent_background: None,
            saved_path: None,
        });

        assert_eq!(
            serde_json::to_value(web_search).expect("web search should serialize"),
            json!({
                "type": "webSearch",
                "id": "search-1",
                "query": "docs",
                "action": null,
                "results": null,
                "source": null,
            })
        );
        assert_eq!(
            serde_json::to_value(image_generation).expect("image generation should serialize"),
            json!({
                "type": "imageGeneration",
                "id": "image-1",
                "status": "completed",
                "revisedPrompt": null,
                "prompt": null,
                "result": "",
                "transparentBackground": null,
                "savedPath": null,
            })
        );
    }
}
