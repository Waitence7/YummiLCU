use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FLIGHT_RECORDS: usize = 2_048;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FlightRecord {
    pub(crate) at_ms: u64,
    pub(crate) category: &'static str,
    pub(crate) detail: String,
}

#[derive(Default)]
pub(crate) struct FlightRecorder {
    records: VecDeque<FlightRecord>,
}

impl FlightRecorder {
    pub(crate) fn record(&mut self, category: &'static str, detail: impl Into<String>) {
        self.records.push_back(FlightRecord {
            at_ms: now_ms(),
            category,
            detail: detail.into(),
        });
        while self.records.len() > MAX_FLIGHT_RECORDS {
            self.records.pop_front();
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<FlightRecord> {
        self.records.iter().cloned().collect()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.records.len()
    }

    #[cfg(test)]
    fn first_detail(&self) -> Option<&str> {
        self.records.front().map(|record| record.detail.as_str())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flight_recorder_is_bounded_and_fifo() {
        let mut recorder = FlightRecorder::default();
        for index in 0..(MAX_FLIGHT_RECORDS + 3) {
            recorder.record("test", format!("event-{index}"));
        }
        assert_eq!(recorder.len(), MAX_FLIGHT_RECORDS);
        assert_eq!(recorder.first_detail(), Some("event-3"));
    }
}
