use crate::sequence::{SfxFrame, SfxSequence, YmChannel, YmFrame, YmSequence};
use std::sync::Arc;
use ym2149::{Ym2149, Ym2149Backend};

#[derive(Clone)]
struct PlayingSfx {
    frames: Arc<[SfxFrame]>,
    current_idx: usize,
}

fn idx_to_ym_channel(idx: usize) -> YmChannel {
    match idx {
        0 => YmChannel::A,
        1 => YmChannel::B,
        _ => YmChannel::C,
    }
}

fn ym_channel_to_idx(ch: YmChannel) -> usize {
    match ch {
        YmChannel::A => 0,
        YmChannel::B => 1,
        YmChannel::C => 2,
    }
}

fn calculate_samples_per_frame(sample_rate: u32, hz: u32) -> usize {
    let hz_valid = hz.max(1);
    (f64::from(sample_rate) / f64::from(hz_valid)).round() as usize
}

/// Clocks `chip` one cycle and writes its output sample into `data[i..]`, replicated
/// across `channels` (e.g. duplicated into both slots of a stereo frame).
fn step_and_write(chip: &mut Ym2149, data: &mut [f32], i: usize, channels: usize) {
    let sample_val = chip.get_sample();
    chip.clock();
    for c in 0..channels {
        if i + c < data.len() {
            data[i + c] = sample_val;
        }
    }
}

/// Rebuilds absolute chip register state by replaying `frames[..=target]` from a fresh
/// chip. Song frames are sparse diffs, so seeking requires replaying every frame's
/// register writes from the start — cheap, since it's just integer writes with no
/// audio synthesis (no `chip.clock()`/`get_sample()` calls).
fn rebuild_chip_state_at(
    frames: &[YmFrame],
    target: usize,
    master_clock_hz: u32,
    output_sample_rate: u32,
) -> (Ym2149, u8, Option<u8>) {
    let mut chip = Ym2149::with_clocks(master_clock_hz, output_sample_rate);
    let mut mixer = 0x3F;
    let mut last_env_shape = None;
    for frame in &frames[..=target] {
        frame.apply_to_chip(&mut chip, &mut mixer, &mut last_env_shape);
    }
    (chip, mixer, last_env_shape)
}

/// Pure YM2149 song rendering engine.
/// Computes PCM audio samples for compiled or decoded [`YmSequence`] music tracks.
pub struct YmSongRenderer {
    chip: Ym2149,
    frames: Arc<[YmFrame]>,
    frame_idx: usize,
    sample_in_frame: usize,
    samples_per_frame: usize,
    mixer: u8,
    last_env_shape: Option<u8>,
    loop_start: Option<usize>,
    master_clock_hz: u32,
    output_sample_rate: u32,
    finished: bool,
}

impl YmSongRenderer {
    /// Creates a new song renderer for `sequence` targeted at `output_sample_rate`.
    #[must_use]
    pub fn new(sequence: &YmSequence, output_sample_rate: u32) -> Self {
        let hz = sequence.timing.frame_rate.hz_value();
        let samples_per_frame = calculate_samples_per_frame(output_sample_rate, hz);
        let mut chip = Ym2149::with_clocks(sequence.timing.master_clock_hz, output_sample_rate);
        let frames: Arc<[YmFrame]> = sequence.frames.as_slice().into();

        let mut mixer = 0x3F;
        let mut last_env_shape = None;
        if !frames.is_empty() {
            frames[0].apply_to_chip(&mut chip, &mut mixer, &mut last_env_shape);
        }

        Self {
            chip,
            frames,
            frame_idx: 0,
            sample_in_frame: 0,
            samples_per_frame,
            mixer,
            last_env_shape,
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
        while i < data.len() {
            step_and_write(&mut self.chip, data, i, channels);
            i += channels;

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
                self.frames[idx].apply_to_chip(
                    &mut self.chip,
                    &mut self.mixer,
                    &mut self.last_env_shape,
                );
            }
        }
    }

    /// Seeks song playback to `target_frame` by replaying state diffs.
    pub fn seek(&mut self, target_frame: usize) {
        if self.frames.is_empty() {
            return;
        }
        let target = target_frame.min(self.frames.len().saturating_sub(1));
        let (chip, mixer, last_env_shape) = rebuild_chip_state_at(
            &self.frames,
            target,
            self.master_clock_hz,
            self.output_sample_rate,
        );

        self.chip = chip;
        self.frame_idx = target;
        self.sample_in_frame = 0;
        self.mixer = mixer;
        self.last_env_shape = last_env_shape;
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

/// Pure YM2149 sound effect sequence rendering engine.
pub struct YmSfxRenderer {
    chip: Ym2149,
    frames: Arc<[SfxFrame]>,
    frame_idx: usize,
    sample_in_frame: usize,
    samples_per_frame: usize,
    mixer: u8,
    channel: YmChannel,
    finished: bool,
}

impl YmSfxRenderer {
    /// Creates a new sound effect renderer for `sequence` targeted at `output_sample_rate`.
    #[must_use]
    pub fn new(sequence: &SfxSequence, output_sample_rate: u32) -> Self {
        let hz = sequence.source_hz;
        let samples_per_frame = calculate_samples_per_frame(output_sample_rate, hz);
        let mut chip = Ym2149::with_clocks(sequence.source_clock, output_sample_rate);
        let frames: Arc<[SfxFrame]> = sequence.frames.as_slice().into();

        let channel = sequence
            .preferred_channels
            .as_ref()
            .and_then(|c| c.first().copied())
            .unwrap_or(YmChannel::A);

        let mut mixer = 0x3F;
        if !frames.is_empty() {
            frames[0].apply_to_chip(&mut chip, &mut mixer, channel);
        }

        Self {
            chip,
            frames,
            frame_idx: 0,
            sample_in_frame: 0,
            samples_per_frame,
            mixer,
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
        while i < data.len() {
            step_and_write(&mut self.chip, data, i, channels);
            i += channels;

            self.sample_in_frame += 1;
            if self.sample_in_frame >= self.samples_per_frame {
                self.sample_in_frame = 0;
                self.frame_idx += 1;

                if self.frame_idx >= total_frames {
                    self.finished = true;
                    return;
                }

                let idx = self.frame_idx;
                self.frames[idx].apply_to_chip(&mut self.chip, &mut self.mixer, self.channel);
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
    pub fn channel(&self) -> YmChannel {
        self.channel
    }
}

/// Pure YM2149 interactive multi-channel song & SFX mixing engine.
pub struct YmMixer {
    chip: Ym2149,
    song_frames: Arc<[YmFrame]>,
    sfx_frames_list: Vec<Arc<[SfxFrame]>>,
    song_frame_idx: usize,
    sample_in_frame: usize,
    samples_per_frame: usize,
    mixer: u8,
    last_env_shape: Option<u8>,
    loop_start: Option<usize>,
    master_clock_hz: u32,
    output_sample_rate: u32,
    preferred_chan_idx: usize,
    active_sfx: [Option<PlayingSfx>; 3],
    finished: bool,
}

impl YmMixer {
    /// Creates a new interactive song & SFX mixer targeted at `output_sample_rate`.
    #[must_use]
    pub fn new(
        song_seq: &YmSequence,
        sfx_list: &[SfxSequence],
        preferred_channel: YmChannel,
        output_sample_rate: u32,
    ) -> Self {
        let song_hz = song_seq.timing.frame_rate.hz_value();
        let samples_per_frame = calculate_samples_per_frame(output_sample_rate, song_hz);
        let mut chip = Ym2149::with_clocks(song_seq.timing.master_clock_hz, output_sample_rate);
        let song_frames: Arc<[YmFrame]> = song_seq.frames.as_slice().into();
        let sfx_frames_list: Vec<Arc<[SfxFrame]>> = sfx_list
            .iter()
            .map(|s| s.frames.as_slice().into())
            .collect();

        let mut mixer = 0x3F;
        let mut last_env_shape = None;
        if !song_frames.is_empty() {
            song_frames[0].apply_to_chip(&mut chip, &mut mixer, &mut last_env_shape);
        }

        Self {
            chip,
            song_frames,
            sfx_frames_list,
            song_frame_idx: 0,
            sample_in_frame: 0,
            samples_per_frame,
            mixer,
            last_env_shape,
            loop_start: song_seq.loop_start.or(Some(0)),
            master_clock_hz: song_seq.timing.master_clock_hz,
            output_sample_rate,
            preferred_chan_idx: ym_channel_to_idx(preferred_channel),
            active_sfx: [None, None, None],
            finished: song_seq.frames.is_empty(),
        }
    }

    /// Triggers SFX index `sfx_idx` to start playing over the music channels.
    pub fn trigger_sfx(&mut self, sfx_idx: usize) {
        let Some(frames) = self.sfx_frames_list.get(sfx_idx) else {
            return;
        };
        let target_ch = if self.active_sfx[self.preferred_chan_idx].is_none() {
            self.preferred_chan_idx
        } else if self.active_sfx[2].is_none() {
            2
        } else if self.active_sfx[1].is_none() {
            1
        } else if self.active_sfx[0].is_none() {
            0
        } else {
            self.preferred_chan_idx
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
        while i < data.len() {
            step_and_write(&mut self.chip, data, i, channels);
            i += channels;

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
                    sf.apply_to_chip(&mut self.chip, &mut self.mixer, &mut self.last_env_shape);
                }

                for ch in 0..3 {
                    if let Some(ref mut active) = self.active_sfx[ch] {
                        if active.current_idx < active.frames.len() {
                            let frame = &active.frames[active.current_idx];
                            frame.apply_to_chip(
                                &mut self.chip,
                                &mut self.mixer,
                                idx_to_ym_channel(ch),
                            );
                            active.current_idx += 1;
                        } else {
                            self.active_sfx[ch] = None;
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
        let (chip, mixer, last_env_shape) = rebuild_chip_state_at(
            &self.song_frames,
            target,
            self.master_clock_hz,
            self.output_sample_rate,
        );

        self.chip = chip;
        self.song_frame_idx = target;
        self.sample_in_frame = 0;
        self.mixer = mixer;
        self.last_env_shape = last_env_shape;
        self.finished = false;
    }

    #[must_use]
    pub fn current_song_frame(&self) -> usize {
        self.song_frame_idx
    }

    #[must_use]
    pub fn total_song_frames(&self) -> usize {
        self.song_frames.len()
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
    }
}

/// Pure YM2149 raw `.ym` file chiptune rendering engine using `ym2149-ym-replayer`.
pub struct YmDataRenderer {
    player: ym2149_ym_replayer::player::ym_player::YmPlayer,
    duration_seconds: f64,
    temp_buf: Vec<f32>,
}

impl YmDataRenderer {
    /// Decodes raw `.ym` data (LHA decompressing if needed) and creates a sample renderer.
    ///
    /// # Errors
    ///
    /// Returns an error if decompressing or parsing the YM header fails.
    pub fn new(
        ym_data: &[u8],
        output_sample_rate: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        use ym2149_common::ChiptunePlayerBase;
        use ym2149_ym_replayer::player::PlaybackController;

        let decompressed = ym2149_ym_replayer::compression::decompress_if_needed(ym_data)?;
        let (mut player, _summary) = ym2149_ym_replayer::player::ym_player::load_song_with_rate(
            &decompressed,
            output_sample_rate,
        )?;

        PlaybackController::play(&mut player)?;
        let duration_seconds = f64::from(player.duration_seconds());

        Ok(Self {
            player,
            duration_seconds,
            temp_buf: vec![0.0f32; 8192],
        })
    }

    /// Renders PCM float samples into `data` buffer.
    pub fn render_samples(&mut self, data: &mut [f32], channels: usize) {
        let needed_len = (data.len() / channels.max(1)).min(self.temp_buf.len());
        let slice = &mut self.temp_buf[..needed_len];
        slice.fill(0.0);
        self.player.generate_samples_into(slice);

        let mut temp_idx = 0;
        for frame in data.chunks_exact_mut(channels.max(1)) {
            if temp_idx < needed_len {
                let sample_val = slice[temp_idx];
                for sample in frame.iter_mut() {
                    *sample = sample_val;
                }
                temp_idx += 1;
            }
        }
    }

    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }
}
