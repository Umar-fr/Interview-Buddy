use super::vad::SpeechSegment;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
pub struct LatestSpeechQueue {
    segment: Mutex<Option<SpeechSegment>>,
    ready: Condvar,
}

impl LatestSpeechQueue {
    pub fn push(&self, segment: SpeechSegment) -> bool {
        let mut queued = self
            .segment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let replaced = queued.replace(segment).is_some();
        self.ready.notify_one();
        replaced
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<SpeechSegment> {
        let queued = self
            .segment
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut queued, _) = self
            .ready
            .wait_timeout_while(queued, timeout, |queued| queued.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queued.take()
    }
}

#[cfg(test)]
mod tests {
    use super::LatestSpeechQueue;
    use crate::stt::vad::SpeechSegment;
    use std::time::Duration;

    fn segment(timestamp_ms: u64) -> SpeechSegment {
        SpeechSegment {
            samples: vec![timestamp_ms as f32],
            timestamp_ms,
        }
    }

    #[test]
    fn replaces_stale_queued_speech_with_the_latest_segment() {
        let queue = LatestSpeechQueue::default();
        assert!(!queue.push(segment(1)));
        assert!(queue.push(segment(2)));
        assert_eq!(
            queue.recv_timeout(Duration::from_millis(1)).unwrap().timestamp_ms,
            2
        );
        assert!(queue.recv_timeout(Duration::from_millis(1)).is_none());
    }
}
