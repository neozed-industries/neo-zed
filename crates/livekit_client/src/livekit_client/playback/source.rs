use std::num::NonZero;

use futures::StreamExt;
use libwebrtc::{audio_stream::native::NativeAudioStream, prelude::AudioFrame};
use livekit::track::RemoteAudioTrack;
use rodio::{
    ChannelCount, SampleRate, Source, buffer::SamplesBuffer, conversions::SampleTypeConverter,
};

use audio::{CHANNEL_COUNT, LEGACY_CHANNEL_COUNT, LEGACY_SAMPLE_RATE, SAMPLE_RATE};

fn frame_to_samplesbuffer(frame: AudioFrame) -> SamplesBuffer {
    let samples = frame.data.iter().copied();
    let samples = SampleTypeConverter::<_, _>::new(samples);
    let samples: Vec<f32> = samples.collect();
    SamplesBuffer::new(
        NonZero::new(frame.num_channels as u16).expect("zero channels is nonsense"),
        NonZero::new(frame.sample_rate).expect("samplerate zero is nonsense"),
        samples,
    )
}

pub struct LiveKitStream {
    // shared_buffer: SharedBuffer,
    inner: rodio::queue::SourcesQueueOutput,
    _receiver_task: gpui::Task<()>,
    channel_count: ChannelCount,
    sample_rate: SampleRate,
}

impl LiveKitStream {
    pub fn new(
        executor: &gpui::BackgroundExecutor,
        track: &RemoteAudioTrack,
        legacy: bool,
    ) -> Self {
        let (channel_count, sample_rate) = if legacy {
            (LEGACY_CHANNEL_COUNT, LEGACY_SAMPLE_RATE)
        } else {
            (CHANNEL_COUNT, SAMPLE_RATE)
        };

        let (queue_input, queue_output) = rodio::queue::queue(true);
        // spawn rtc stream
        let receiver_task = executor.spawn_with_priority(gpui::Priority::RealtimeAudio, {
            let rtc_track = track.rtc_track();
            let sample_rate_i32 = sample_rate.get() as i32;
            let channel_count_i32 = channel_count.get().into();
            async move {
                // NativeAudioStream::new calls AudioTrack::add_sink which blocks on the WebRTC
                // signaling thread. Doing this inside the background task avoids blocking the
                // main thread when the signaling thread is busy (e.g. during reconnection).
                let mut stream =
                    NativeAudioStream::new(rtc_track, sample_rate_i32, channel_count_i32);
                while let Some(frame) = stream.next().await {
                    let samples = frame_to_samplesbuffer(frame);
                    queue_input.append(samples);
                }
            }
        });

        LiveKitStream {
            _receiver_task: receiver_task,
            inner: queue_output,
            sample_rate,
            channel_count,
        }
    }
}

impl Iterator for LiveKitStream {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl Source for LiveKitStream {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.channel_count
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}
