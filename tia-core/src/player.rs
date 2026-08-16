use crate::chip::{TiaChannel, TiaChip};
use crate::sequence::{TiaFrame, TiaSequence, TiaSfxFrame, TiaSfxSequence};
use std::sync::Arc;

#[derive(Clone)]
struct PlayingSfx {
    frames: Arc<[TiaSfxFrame]>,
    current_idx: usize,
}

fn calculate_samples_per_frame(sample_rate: u32, hz: u32) -> usize {
    let hz_valid = hz.max(1);
    (f64::from(sample_rate) / f64::from(hz_valid)).round() as usize
}

/// Clocks `chip` and writes its output sample into `data[i..]`, replicated across `channels`.
fn step_and_write(chip: &mut TiaChip, data: &mut [f32], i: usize, channels: usize) {
    if channels == 1 {
        if i < data.len() {
            data[i] = chip.get_sample();
        }
    } else if channels == 2 {
        let (l, r) = chip.get_stereo_sample();
        if i < data.len() {
            data[i] = l;
        }
        if i + 1 < data.len() {
            data[i + 1] = r;
        }
    } else {
        let sample_val = chip.get_sample();
        for c in 0..channels {
            if i + c < data.len() {
                data[i + c] = sample_val;
            }
        }
    }
}

/// Rebuilds chip register state by replaying frames up to `target`.
fn rebuild_chip_state_at(
    frames: &[TiaFrame],
    target: usize,
    master_clock_hz: u32,
    output_sample_rate: u32,
) -> TiaChip {
    let mut chip = TiaChip::new(master_clock_hz, output_sample_rate);
    for frame in &frames[..=target] {
        frame.apply_to_chip(&mut chip);
    }
    chip
}

/// High-performance TIA song rendering engine.
pub struct TiaSongRenderer {
    chip: TiaChip,
    frames: Arc<[TiaFrame]>,
    frame_idx: usize,
    sample_in_frame: usize,
    samples_per_frame: usize,
    loop_start: Option<usize>,
    master_clock_hz: u32,
    output_sample_rate: u32,
    finished: bool,
}

impl TiaSongRenderer {
    /// Creates a new song renderer for `sequence` targeted at `output_sample_rate`.
    #[must_use]
    pub fn new(sequence: &TiaSequence, output_sample_rate: u32) -> Self {
        let hz = sequence.timing.frame_rate.hz_value();
        let samples_per_frame = calculate_samples_per_frame(output_sample_rate, hz);
        let mut chip = TiaChip::new(sequence.timing.master_clock_hz, output_sample_rate);
        let frames: Arc<[TiaFrame]> = sequence.frames.as_slice().into();

        if !frames.is_empty() {
            frames[0].apply_to_chip(&mut chip);
        }

        Self {
            chip,
            frames,
            frame_idx: 0,
            sample_in_frame: 0,
            samples_per_frame,
            loop_start: sequence.loop_start,
            master_clock_hz: sequence.timing.master_clock_hz,
            output_sample_rate,
            finished: sequence.frames.is_empty(),
        }
    }

    /// Renders PCM float samples into `data` buffer.
    pub fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        if self.finished {
            data.fill(0.0);
            return;
        }

        let total_frames = self.frames.len();
        let mut i = 0;
        let step = channels.max(1);

        while i < data.len() {
            step_and_write(&mut self.chip, data, i, channels);
            i += step;

            self.sample_in_frame += 1;
            if self.sample_in_frame >= self.samples_per_frame {
                self.sample_in_frame = 0;
                self.frame_idx += 1;

                if self.frame_idx >= total_frames {
                    if let Some(l_start) = self.loop_start {
                        self.frame_idx = l_start;
                    } else {
                        self.finished = true;
                        return;
                    }
                }

                let idx = self.frame_idx;
                if idx < self.frames.len() {
                    self.frames[idx].apply_to_chip(&mut self.chip);
                }
            }
        }
    }

    /// Seeks song playback to `target_frame`.
    pub fn seek(&mut self, target_frame: usize) {
        if self.frames.is_empty() {
            return;
        }
        let target = target_frame.min(self.frames.len().saturating_sub(1));
        self.chip = rebuild_chip_state_at(
            &self.frames,
            target,
            self.master_clock_hz,
            self.output_sample_rate,
        );
        self.frame_idx = target;
        self.sample_in_frame = 0;
        self.finished = false;
    }

    #[must_use]
    pub fn current_frame(&self) -> usize {
        self.frame_idx
    }

    #[must_use]
    pub fn total_frames(&self) -> usize {
        self.frames.len()
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
    }

    #[must_use]
    pub fn is_looping(&self) -> bool {
        self.loop_start.is_some()
    }
}

/// Pure TIA sound effect sequence rendering engine.
pub struct TiaSfxRenderer {
    chip: TiaChip,
    frames: Arc<[TiaSfxFrame]>,
    frame_idx: usize,
    sample_in_frame: usize,
    samples_per_frame: usize,
    channel: TiaChannel,
    finished: bool,
}

impl TiaSfxRenderer {
    /// Creates a new sound effect renderer for `sequence` targeted at `output_sample_rate`.
    #[must_use]
    pub fn new(sequence: &TiaSfxSequence, output_sample_rate: u32) -> Self {
        let hz = sequence.source_hz;
        let samples_per_frame = calculate_samples_per_frame(output_sample_rate, hz);
        let mut chip = TiaChip::new(sequence.source_clock, output_sample_rate);
        let frames: Arc<[TiaSfxFrame]> = sequence.frames.as_slice().into();
        let channel = sequence.preferred_channel.unwrap_or(TiaChannel::Ch0);

        if !frames.is_empty() {
            frames[0].apply_to_chip(&mut chip, channel);
        }

        Self {
            chip,
            frames,
            frame_idx: 0,
            sample_in_frame: 0,
            samples_per_frame,
            channel,
            finished: sequence.frames.is_empty(),
        }
    }

    /// Renders PCM float samples into `data` buffer.
    pub fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        if self.finished {
            data.fill(0.0);
            return;
        }

        let total_frames = self.frames.len();
        let mut i = 0;
        let step = channels.max(1);

        while i < data.len() {
            step_and_write(&mut self.chip, data, i, channels);
            i += step;

            self.sample_in_frame += 1;
            if self.sample_in_frame >= self.samples_per_frame {
                self.sample_in_frame = 0;
                self.frame_idx += 1;

                if self.frame_idx >= total_frames {
                    self.finished = true;
                    return;
                }

                let idx = self.frame_idx;
                if idx < self.frames.len() {
                    self.frames[idx].apply_to_chip(&mut self.chip, self.channel);
                }
            }
        }
    }

    #[must_use]
    pub fn current_frame(&self) -> usize {
        self.frame_idx
    }

    #[must_use]
    pub fn total_frames(&self) -> usize {
        self.frames.len()
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
    }

    #[must_use]
    pub fn channel(&self) -> TiaChannel {
        self.channel
    }
}

/// Pure TIA interactive multi-channel song & SFX mixing engine.
pub struct TiaMixer {
    chip: TiaChip,
    song_frames: Arc<[TiaFrame]>,
    sfx_frames_list: Vec<Arc<[TiaSfxFrame]>>,
    song_frame_idx: usize,
    sample_in_frame: usize,
    samples_per_frame: usize,
    loop_start: Option<usize>,
    master_clock_hz: u32,
    output_sample_rate: u32,
    preferred_channel: TiaChannel,
    active_sfx: [Option<PlayingSfx>; 2],
    finished: bool,
}

impl TiaMixer {
    /// Creates a new interactive song & SFX mixer targeted at `output_sample_rate`.
    #[must_use]
    pub fn new(
        song_seq: &TiaSequence,
        sfx_list: &[TiaSfxSequence],
        preferred_channel: TiaChannel,
        output_sample_rate: u32,
    ) -> Self {
        let song_hz = song_seq.timing.frame_rate.hz_value();
        let samples_per_frame = calculate_samples_per_frame(output_sample_rate, song_hz);
        let mut chip = TiaChip::new(song_seq.timing.master_clock_hz, output_sample_rate);
        let song_frames: Arc<[TiaFrame]> = song_seq.frames.as_slice().into();
        let sfx_frames_list: Vec<Arc<[TiaSfxFrame]>> = sfx_list
            .iter()
            .map(|s| s.frames.as_slice().into())
            .collect();

        if !song_frames.is_empty() {
            song_frames[0].apply_to_chip(&mut chip);
        }

        Self {
            chip,
            song_frames,
            sfx_frames_list,
            song_frame_idx: 0,
            sample_in_frame: 0,
            samples_per_frame,
            loop_start: song_seq.loop_start.or(Some(0)),
            master_clock_hz: song_seq.timing.master_clock_hz,
            output_sample_rate,
            preferred_channel,
            active_sfx: [None, None],
            finished: song_seq.frames.is_empty(),
        }
    }

    /// Triggers SFX index `sfx_idx` to play over the music channels.
    pub fn trigger_sfx(&mut self, sfx_idx: usize) {
        let Some(frames) = self.sfx_frames_list.get(sfx_idx) else {
            return;
        };
        let pref_idx = self.preferred_channel.index();
        let other_idx = 1 - pref_idx;

        let target_ch = if self.active_sfx[pref_idx].is_none() {
            pref_idx
        } else if self.active_sfx[other_idx].is_none() {
            other_idx
        } else {
            pref_idx
        };

        self.active_sfx[target_ch] = Some(PlayingSfx {
            frames: Arc::clone(frames),
            current_idx: 0,
        });
    }

    /// Renders PCM float samples into `data` buffer.
    pub fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        if self.finished {
            data.fill(0.0);
            return;
        }

        let total_song_frames = self.song_frames.len();
        let mut i = 0;
        let step = channels.max(1);

        while i < data.len() {
            step_and_write(&mut self.chip, data, i, channels);
            i += step;

            self.sample_in_frame += 1;
            if self.sample_in_frame >= self.samples_per_frame {
                self.sample_in_frame = 0;
                self.song_frame_idx += 1;

                if self.song_frame_idx >= total_song_frames {
                    if let Some(l_start) = self.loop_start {
                        self.song_frame_idx = l_start;
                    } else {
                        self.finished = true;
                        return;
                    }
                }

                let song_idx = self.song_frame_idx;
                if let Some(sf) = self.song_frames.get(song_idx) {
                    sf.apply_to_chip(&mut self.chip);
                }

                for ch_idx in 0..2 {
                    if let Some(ref mut active) = self.active_sfx[ch_idx] {
                        if active.current_idx < active.frames.len() {
                            let frame = &active.frames[active.current_idx];
                            frame.apply_to_chip(&mut self.chip, TiaChannel::from_index(ch_idx));
                            active.current_idx += 1;
                        } else {
                            self.active_sfx[ch_idx] = None;
                        }
                    }
                }
            }
        }
    }

    /// Seeks song playback to `target_frame`.
    pub fn seek_song(&mut self, target_frame: usize) {
        if self.song_frames.is_empty() {
            return;
        }
        let target = target_frame.min(self.song_frames.len().saturating_sub(1));
        self.chip = rebuild_chip_state_at(
            &self.song_frames,
            target,
            self.master_clock_hz,
            self.output_sample_rate,
        );
        self.song_frame_idx = target;
        self.sample_in_frame = 0;
        self.finished = false;
    }

    #[must_use]
    pub fn song_current_frame(&self) -> usize {
        self.song_frame_idx
    }

    #[must_use]
    pub fn song_total_frames(&self) -> usize {
        self.song_frames.len()
    }

    #[must_use]
    pub fn is_song_finished(&self) -> bool {
        self.finished
    }

    #[must_use]
    pub fn is_sfx_active(&self, ch: usize) -> bool {
        if ch < 2 {
            self.active_sfx[ch].is_some()
        } else {
            false
        }
    }
}
