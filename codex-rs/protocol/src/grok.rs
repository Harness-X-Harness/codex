/// Returns whether `name` is one of the X Search tool names established by
/// the Grok Gateway wire contract.
pub fn is_evidence_backed_x_search_name(name: &str) -> bool {
    matches!(
        name,
        "x_keyword_search" | "x_semantic_search" | "x_user_search" | "x_thread_fetch"
    )
}
