//! Ink queries on the shared telemetry, a child module so `capture.rs` stays under the code-shape
//! cap.

impl super::SharedCaptureTelemetry {
    /// The end time of the newest lifted ink stroke, without copying the
    /// strokes (no laser).
    pub fn newest_ink_end_ms(&self) -> Option<u64> {
        self.strokes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .rev()
            .find(|stroke| !stroke.laser)
            .map(|stroke| stroke.t1)
    }

    /// Whether a stroke is still being drawn, without copying it.
    pub fn has_live_ink(&self) -> bool {
        self.live_ink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|stroke| !stroke.pts.is_empty())
    }
}
