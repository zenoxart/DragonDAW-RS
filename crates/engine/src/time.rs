//! Tempo map: converts between beats, seconds and sample frames, honouring tempo changes.

use crate::model::Project;

#[derive(Debug, Clone, Copy)]
struct Segment {
    beat: f64,
    seconds: f64,
    bpm: f64,
}

/// A step tempo map (constant tempo between points).
#[derive(Debug, Clone)]
pub struct TempoMap {
    segments: Vec<Segment>,
}

impl TempoMap {
    pub fn constant(bpm: f64) -> Self {
        TempoMap { segments: vec![Segment { beat: 0.0, seconds: 0.0, bpm: bpm.clamp(10.0, 999.0) }] }
    }

    pub fn from_project(p: &Project) -> Self {
        let mut points: Vec<(f64, f64)> = p.tempo_points.iter().filter(|t| t.beat > 0.0).map(|t| (t.beat, t.bpm.clamp(10.0, 999.0))).collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let first = p.tempo_points.iter().find(|t| t.beat <= 0.0).map(|t| t.bpm).unwrap_or(p.bpm).clamp(10.0, 999.0);
        let mut segments = vec![Segment { beat: 0.0, seconds: 0.0, bpm: first }];
        for (beat, bpm) in points {
            let last = *segments.last().unwrap();
            let seconds = last.seconds + (beat - last.beat) * 60.0 / last.bpm;
            segments.push(Segment { beat, seconds, bpm });
        }
        TempoMap { segments }
    }

    fn seg_by_beat(&self, beat: f64) -> &Segment {
        let i = self.segments.partition_point(|s| s.beat <= beat).max(1);
        &self.segments[i - 1]
    }

    fn seg_by_seconds(&self, seconds: f64) -> &Segment {
        let i = self.segments.partition_point(|s| s.seconds <= seconds).max(1);
        &self.segments[i - 1]
    }

    pub fn bpm_at(&self, beat: f64) -> f64 {
        self.seg_by_beat(beat).bpm
    }

    pub fn beat_to_seconds(&self, beat: f64) -> f64 {
        let s = self.seg_by_beat(beat);
        s.seconds + (beat - s.beat) * 60.0 / s.bpm
    }

    pub fn seconds_to_beat(&self, seconds: f64) -> f64 {
        let s = self.seg_by_seconds(seconds);
        s.beat + (seconds - s.seconds) * s.bpm / 60.0
    }

    pub fn beat_to_frame(&self, beat: f64, rate: f64) -> f64 {
        self.beat_to_seconds(beat) * rate
    }

    pub fn frame_to_beat(&self, frame: f64, rate: f64) -> f64 {
        self.seconds_to_beat(frame / rate)
    }
}

/// Formats a beat as `bar:beat:tick` (1-based, 4/4 assumed by the caller's beats-per-bar).
pub fn bbt(beat: f64, beats_per_bar: f64) -> String {
    let beat = beat.max(0.0);
    let bar = (beat / beats_per_bar).floor();
    let in_bar = beat - bar * beats_per_bar;
    let b = in_bar.floor();
    let tick = ((in_bar - b) * 96.0).round() as i64;
    format!("{}:{}:{:02}", bar as i64 + 1, b as i64 + 1, tick.min(95))
}

/// Formats seconds as `m:ss.mmm`.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let m = (s / 60.0).floor();
    format!("{}:{:06.3}", m as i64, s - m * 60.0)
}

/// SMPTE timecode `hh:mm:ss:ff`.
pub fn timecode(seconds: f64, fps: f64) -> String {
    let s = seconds.max(0.0);
    let frames_total = (s * fps).floor() as i64;
    let fps_i = fps.round() as i64;
    let ff = frames_total % fps_i;
    let total_s = frames_total / fps_i;
    format!("{:02}:{:02}:{:02}:{:02}", total_s / 3600, (total_s / 60) % 60, total_s % 60, ff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, TempoPoint};

    #[test]
    fn constant_tempo() {
        let m = TempoMap::constant(120.0);
        assert!((m.beat_to_seconds(4.0) - 2.0).abs() < 1e-9);
        assert!((m.seconds_to_beat(1.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn tempo_change() {
        let mut p = Project { bpm: 120.0, ..Project::default() };
        p.tempo_points.push(TempoPoint { beat: 4.0, bpm: 60.0 });
        let m = TempoMap::from_project(&p);
        assert!((m.beat_to_seconds(4.0) - 2.0).abs() < 1e-9);
        assert!((m.beat_to_seconds(5.0) - 3.0).abs() < 1e-9);
        assert!((m.seconds_to_beat(3.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn formats() {
        assert_eq!(bbt(0.0, 4.0), "1:1:00");
        assert_eq!(bbt(5.5, 4.0), "2:2:48");
        assert_eq!(clock(61.5), "1:01.500");
        assert_eq!(timecode(3661.5, 25.0), "01:01:01:12");
    }
}
