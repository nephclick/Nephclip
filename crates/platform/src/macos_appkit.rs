//! Native macOS AppKit integration ("Speak Mac"): Taptic Engine haptic feedback for magnetic
//! timeline snaps (same native feel as Final Cut Pro).

#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

use objc2_app_kit::{NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer};

/// Fire a crisp native macOS Alignment haptic tap on the MacBook Force Touch trackpad when clips
/// or the playhead snap magnetically on the Timeline.
pub fn haptic_alignment() {
    let performer = NSHapticFeedbackManager::defaultPerformer();
    performer.performFeedbackPattern_performanceTime(NSHapticFeedbackPattern::Alignment, NSHapticFeedbackPerformanceTime::Now);
}
