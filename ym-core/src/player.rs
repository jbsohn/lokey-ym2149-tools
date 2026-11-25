use crate::sequence::{SfxFrame, SfxSequence, YmChannel, YmFrame, YmSequence};
use crate::timing::TimingConfig;
use std::sync::Arc;
use ym2149::{Ym2149, Ym2149Backend};

#[derive(Clone)]
pub(crate) struct PlayingSfx {
    frames: Arc<[SfxFrame]>,
    current_idx: usize,
    ticks_remaining: u8,
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

/// Pure mono YM-2149 song rendering engine.
/// Computes mono PCM audio samples for compiled or decoded [`YmSequence`] music tracks.
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
    pub fn new(sequence: &YmSequence, output_sample_rate: u32) -> Self {
        let samples_per_frame = sequence.timing.samples_per_frame(output_sample_rate);
        let mut chip = Ym2149::with_clocks(sequence.timing.master_clock_hz, output_sample_rate);
        let frames: Arc<[YmFrame]> = sequence.frames.as_slice().into();

        let mut mixer = 0x3F;
        let mut last_env_shape = None;
        if let Some(first) = frames.first() {
            first.apply_to_chip(&mut chip, &mut mixer, &mut last_env_shape);
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

    /// Renders mono PCM float samples into `data`.
    pub fn render_samples(&mut self, data: &mut [f32]) {
        if self.finished {
            data.fill(0.0);
            return;
        }

        let total_frames = self.frames.len();
        for sample in data.iter_mut() {
            if self.finished {
                *sample = 0.0;
                continue;
            }

            *sample = self.chip.get_sample();
            self.chip.clock();

            self.sample_in_frame += 1;
            if self.sample_in_frame >= self.samples_per_frame {
                self.sample_in_frame = 0;
                self.frame_idx += 1;

                if self.frame_idx >= total_frames {
                    if let Some(l_start) = self.loop_start {
                        self.frame_idx = l_start;
                    } else {
                        self.finished = true;
                        continue;
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

    pub fn current_frame(&self) -> usize {
        self.frame_idx
    }

    pub fn total_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
    }

    pub fn is_looping(&self) -> bool {
        self.loop_start.is_some()
    }
}

/// Pure mono YM-2149 sound effect sequence rendering engine.
pub struct YmSfxRenderer {
    chip: Ym2149,
    frames: Arc<[SfxFrame]>,
    frame_idx: usize,
    ticks_remaining: u8,
    sample_in_frame: usize,
    samples_per_frame: usize,
    mixer: u8,
    channel: YmChannel,
    finished: bool,
}

impl YmSfxRenderer {
    /// Creates a new sound effect renderer with explicit channel and playback timing.
    pub fn new(
        sequence: &SfxSequence,
        channel: YmChannel,
        timing: TimingConfig,
        output_sample_rate: u32,
    ) -> Self {
        let samples_per_frame = timing.samples_per_frame(output_sample_rate);
        let mut chip = Ym2149::with_clocks(timing.master_clock_hz, output_sample_rate);
        let frames: Arc<[SfxFrame]> = sequence.frames.as_slice().into();

        let mut mixer = 0x3F;
        if let Some(first) = frames.first() {
            first.apply_to_chip(&mut chip, &mut mixer, channel);
        }

        let ticks_remaining = frames.first().map_or(1, |f| f.tick_count());

        Self {
            chip,
            frames,
            frame_idx: 0,
            ticks_remaining,
            sample_in_frame: 0,
            samples_per_frame,
            mixer,
            channel,
            finished: sequence.frames.is_empty(),
        }
    }

    /// Renders mono PCM float samples into `data`.
    pub fn render_samples(&mut self, data: &mut [f32]) {
        if self.finished {
            data.fill(0.0);
            return;
        }

        let total_frames = self.frames.len();
        for sample in data.iter_mut() {
            if self.finished {
                *sample = 0.0;
                continue;
            }

            *sample = self.chip.get_sample();
            self.chip.clock();

            self.sample_in_frame += 1;
            if self.sample_in_frame >= self.samples_per_frame {
                self.sample_in_frame = 0;

                if self.ticks_remaining > 1 {
                    self.ticks_remaining -= 1;
                    continue;
                }

                self.frame_idx += 1;
                if self.frame_idx >= total_frames {
                    self.finished = true;
                    continue;
                }

                let frame = self.frames[self.frame_idx];
                self.ticks_remaining = frame.tick_count();
                frame.apply_to_chip(&mut self.chip, &mut self.mixer, self.channel);
            }
        }
    }

    pub fn current_frame(&self) -> usize {
        self.frame_idx
    }

    pub fn total_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
    }

    pub fn channel(&self) -> YmChannel {
        self.channel
    }
}

/// Pure mono YM-2149 interactive multi-channel song & SFX mixing engine.
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
    pub fn new(
        song_seq: &YmSequence,
        sfx_list: &[SfxSequence],
        preferred_channel: YmChannel,
        output_sample_rate: u32,
    ) -> Self {
        let samples_per_frame = song_seq.timing.samples_per_frame(output_sample_rate);
        let mut chip = Ym2149::with_clocks(song_seq.timing.master_clock_hz, output_sample_rate);
        let song_frames: Arc<[YmFrame]> = song_seq.frames.as_slice().into();
        let sfx_frames_list: Vec<Arc<[SfxFrame]>> = sfx_list
            .iter()
            .map(|s| s.frames.as_slice().into())
            .collect();

        let mut mixer = 0x3F;
        let mut last_env_shape = None;
        if let Some(first) = song_frames.first() {
            first.apply_to_chip(&mut chip, &mut mixer, &mut last_env_shape);
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
            preferred_chan_idx: preferred_channel.to_index(),
            active_sfx: [None, None, None],
            finished: song_seq.frames.is_empty(),
        }
    }

    /// Picks which channel a newly triggered SFX should play on: `preferred` if it's
    /// free, otherwise the first free channel trying C, then B, then A, otherwise
    /// (all three busy) `preferred` itself, cutting off whatever was already playing there.
    pub fn pick_sfx_channel(&self, preferred: usize) -> usize {
        Self::pick_sfx_channel_slot(&self.active_sfx, preferred)
    }

    /// Helper for channel selection across a 3-channel active SFX array.
    pub(crate) fn pick_sfx_channel_slot(
        active: &[Option<PlayingSfx>; 3],
        preferred: usize,
    ) -> usize {
        if active[preferred].is_none() {
            preferred
        } else if active[2].is_none() {
            2
        } else if active[1].is_none() {
            1
        } else if active[0].is_none() {
            0
        } else {
            preferred
        }
    }

    /// Triggers SFX index `sfx_idx` to start playing over the music channels.
    pub fn trigger_sfx(&mut self, sfx_idx: usize) {
        let Some(frames) = self.sfx_frames_list.get(sfx_idx) else {
            return;
        };
        let target_ch = self.pick_sfx_channel(self.preferred_chan_idx);
        let ticks_remaining = frames.first().map_or(1, |f| f.tick_count());

        self.active_sfx[target_ch] = Some(PlayingSfx {
            frames: Arc::clone(frames),
            current_idx: 0,
            ticks_remaining,
        });
    }

    /// Renders mono PCM float samples into `data`.
    pub fn render_samples(&mut self, data: &mut [f32]) {
        if self.finished {
            data.fill(0.0);
            return;
        }

        let total_song_frames = self.song_frames.len();
        for sample in data.iter_mut() {
            if self.finished {
                *sample = 0.0;
                continue;
            }

            *sample = self.chip.get_sample();
            self.chip.clock();

            self.sample_in_frame += 1;
            if self.sample_in_frame >= self.samples_per_frame {
                self.sample_in_frame = 0;
                self.song_frame_idx += 1;

                if self.song_frame_idx >= total_song_frames {
                    if let Some(l_start) = self.loop_start {
                        self.song_frame_idx = l_start;
                    } else {
                        self.finished = true;
                        continue;
                    }
                }

                if let Some(sf) = self.song_frames.get(self.song_frame_idx) {
                    sf.apply_to_chip(&mut self.chip, &mut self.mixer, &mut self.last_env_shape);
                }

                for ch in 0..3 {
                    if let Some(ref mut active) = self.active_sfx[ch] {
                        if let Some(&frame) = active.frames.get(active.current_idx) {
                            frame.apply_to_chip(
                                &mut self.chip,
                                &mut self.mixer,
                                YmChannel::from_index(ch),
                            );
                            if active.ticks_remaining > 1 {
                                active.ticks_remaining -= 1;
                            } else {
                                active.current_idx += 1;
                                if let Some(&next) = active.frames.get(active.current_idx) {
                                    active.ticks_remaining = next.tick_count();
                                } else {
                                    self.active_sfx[ch] = None;
                                }
                            }
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

    pub fn current_song_frame(&self) -> usize {
        self.song_frame_idx
    }

    pub fn total_song_frames(&self) -> usize {
        self.song_frames.len()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn set_finished(&mut self, finished: bool) {
        self.finished = finished;
    }
}

/// Pure mono raw `.ym` file chiptune rendering engine using `ym2149-ym-replayer`.
pub struct YmDataRenderer {
    player: ym2149_ym_replayer::player::ym_player::YmPlayer,
    duration_seconds: f64,
}

impl YmDataRenderer {
    /// Decodes raw `.ym` data (LHA decompressing if needed) and creates a sample renderer.
    ///
    /// # Errors
    /// Returns an error if decompressing or parsing the YM header fails.
    pub fn new(ym_data: &[u8], output_sample_rate: u32) -> Result<Self, crate::error::YmError> {
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
        })
    }

    /// Renders mono PCM float samples into `data`.
    pub fn render_samples(&mut self, data: &mut [f32]) {
        data.fill(0.0);
        self.player.generate_samples_into(data);
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy_slot() -> PlayingSfx {
        PlayingSfx {
            frames: Arc::from(Vec::<SfxFrame>::new()),
            current_idx: 0,
            ticks_remaining: 1,
        }
    }

    #[test]
    fn pick_sfx_channel_prefers_preferred_when_free() {
        let active = [None, None, None];
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, 0), 0);
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, 1), 1);
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, 2), 2);
    }

    #[test]
    fn pick_sfx_channel_steals_c_then_b_then_a_when_preferred_busy() {
        let preferred = 0; // channel A

        // A (preferred) busy, C and B free -> steals C first.
        let active = [Some(busy_slot()), None, None];
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, preferred), 2);

        // A and C busy, B free -> steals B.
        let active = [Some(busy_slot()), None, Some(busy_slot())];
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, preferred), 1);
    }

    #[test]
    fn pick_sfx_channel_falls_back_to_preferred_when_all_busy() {
        let active = [Some(busy_slot()), Some(busy_slot()), Some(busy_slot())];
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, 0), 0);
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, 1), 1);
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, 2), 2);
    }

    #[test]
    fn pick_sfx_channel_skips_to_b_when_preferred_is_c() {
        let preferred = 2;

        let active = [None, None, Some(busy_slot())];
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, preferred), 1);

        let active = [None, Some(busy_slot()), Some(busy_slot())];
        assert_eq!(YmMixer::pick_sfx_channel_slot(&active, preferred), 0);
    }
}
