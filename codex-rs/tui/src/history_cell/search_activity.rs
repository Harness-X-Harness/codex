//! Provider-neutral, render-only search progress beside the canonical transcript.
use super::*;
use codex_app_server_protocol::SearchActivityKind;

#[derive(Debug)]
pub(crate) struct SearchActivityCell {
    item_id: String,
    kind: SearchActivityKind,
    completed: bool,
    start_time: Instant,
    animations_enabled: bool,
}

impl SearchActivityCell {
    pub(crate) fn new(item_id: String, kind: SearchActivityKind, animations_enabled: bool) -> Self {
        Self {
            item_id,
            kind,
            completed: false,
            start_time: Instant::now(),
            animations_enabled,
        }
    }

    pub(crate) fn item_id(&self) -> &str {
        &self.item_id
    }

    pub(crate) fn kind(&self) -> SearchActivityKind {
        self.kind
    }

    pub(crate) fn complete(&mut self) {
        self.completed = true;
    }

    fn summary(&self) -> Line<'static> {
        Line::from(
            match (self.kind, self.completed) {
                (SearchActivityKind::Web, false) => "Searching the web",
                (SearchActivityKind::Web, true) => "Web search completed",
                (SearchActivityKind::X, false) => "Searching X",
                (SearchActivityKind::X, true) => "X search completed",
            }
            .bold(),
        )
    }
}

impl HistoryCell for SearchActivityCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let bullet = if self.completed {
            "•".dim()
        } else {
            activity_indicator(
                Some(self.start_time),
                MotionMode::from_animations_enabled(self.animations_enabled),
                ReducedMotionIndicator::StaticBullet,
            )
            .unwrap_or_else(|| "•".dim())
        };
        let mut line = Line::from(vec![bullet, " ".into()]);
        line.extend(self.summary());
        vec![
            crate::line_truncation::truncate_line_with_ellipsis_if_overflow(
                line,
                usize::from(width),
            ),
        ]
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        PrefixedWrappedHistoryCell::new(self.summary(), vec!["• ".dim()], "  ").display_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(vec![self.summary()])
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        (!self.completed && self.animations_enabled)
            .then(|| u64::try_from(self.start_time.elapsed().as_millis() / 100).unwrap_or(u64::MAX))
    }
}
