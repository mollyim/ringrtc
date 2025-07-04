//
// Copyright 2020-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use log::info;
use ringrtc::{
    common::units::DataRate,
    core::{
        call_mutex::CallMutex,
        group_call::{
            self, ClientId, ConnectionState, EndReason, HttpSfuClient, JoinState, Reaction,
            RemoteDeviceState, RemoteDevicesChangedReason, SpeechEvent,
        },
    },
    lite::{
        http::sim as sim_http,
        sfu::{DemuxId, MemberMap, ObfuscatedResolver, PeekInfo, UserId},
    },
    protobuf,
    webrtc::{
        media::{VideoFrame, VideoFrameMetadata, VideoPixelFormat, VideoSink, VideoTrack},
        peer_connection::{AudioLevel, ReceivedAudioLevel, SendRates},
        peer_connection_factory::{self, PeerConnectionFactory},
    },
};

#[derive(Clone, Default)]
struct Observer {
    remote_devices: Arc<Mutex<Vec<group_call::RemoteDeviceState>>>,
    last_frame_metadata_by_demux_id: Arc<Mutex<HashMap<DemuxId, VideoFrameMetadata>>>,
}

impl group_call::Observer for Observer {
    fn request_membership_proof(&self, _client_id: ClientId) {
        // Should be done before starting
    }

    fn request_group_members(&self, _client_id: ClientId) {
        // Done via handle_peek_changed
    }

    fn handle_connection_state_changed(
        &self,
        _client_id: ClientId,
        connection_state: ConnectionState,
    ) {
        info!("Connection state changed to {:?}", connection_state);
    }

    fn handle_join_state_changed(&self, _client_id: ClientId, join_state: JoinState) {
        info!("Join state changed to {:?}", join_state);
    }

    fn handle_remote_devices_changed(
        &self,
        _client_id: ClientId,
        remote_devices: &[RemoteDeviceState],
        _reason: RemoteDevicesChangedReason,
    ) {
        info!("Remote devices changed to {:?}", remote_devices);
        *self.remote_devices.lock().unwrap() = remote_devices.to_vec();
    }

    fn handle_peek_changed(
        &self,
        _client_id: ClientId,
        peek_info: &PeekInfo,
        _joined_members: &HashSet<UserId>,
    ) {
        info!(
            "Peek info changed to creator: {:?}, era: {:?} devices: {:?}/{:?} {:?}",
            peek_info.creator,
            peek_info.era_id,
            peek_info.device_count_including_pending_devices(),
            peek_info.max_devices,
            peek_info.devices,
        );
    }

    fn send_signaling_message(
        &mut self,
        _recipient_id: UserId,
        _message: ringrtc::protobuf::signaling::CallMessage,
        _urgency: ringrtc::core::group_call::SignalingMessageUrgency,
    ) {
        // This isn't going to work :(.  Better turn off frame crypto.
    }

    fn send_signaling_message_to_group(
        &mut self,
        _group: group_call::GroupId,
        _message: protobuf::signaling::CallMessage,
        _urgency: group_call::SignalingMessageUrgency,
        _recipients_override: HashSet<UserId>,
    ) {
        unimplemented!()
    }

    fn handle_incoming_video_track(
        &mut self,
        _client_id: ClientId,
        sender_demux_id: DemuxId,
        _incoming_video_track: VideoTrack,
    ) {
        info!("Got a video track for {}", sender_demux_id);
    }

    fn handle_ended(&self, _client_id: ClientId, reason: EndReason) {
        info!("Ended with reason {:?}", reason);
    }

    fn handle_network_route_changed(
        &self,
        _client_id: ClientId,
        _network_route: ringrtc::webrtc::peer_connection_observer::NetworkRoute,
    ) {
        // ignore
    }

    fn handle_speaking_notification(&mut self, _client_id: ClientId, event: SpeechEvent) {
        info!("Speaking {:?}", event);
    }

    fn handle_audio_levels(
        &self,
        _client_id: group_call::ClientId,
        _captured_level: AudioLevel,
        _received_levels: Vec<ReceivedAudioLevel>,
    ) {
        // ignore
    }

    fn handle_low_bandwidth_for_video(&self, _client_id: ClientId, _recovered: bool) {
        // ignore
    }

    fn handle_reactions(&self, _client_id: ClientId, _reactions: Vec<Reaction>) {
        // ignore
    }

    fn handle_raised_hands(&self, _client_id: ClientId, raised_hands: Vec<DemuxId>) {
        info!("Raised hands changed to {:?}", raised_hands);
    }

    fn handle_rtc_stats_report(&self, _report_json: String) {
        // ignore
    }

    fn handle_remote_mute_request(&self, _client_id: ClientId, _mute_source: DemuxId) {
        // ignore
    }

    fn handle_observed_remote_mute(
        &self,
        _client_id: ClientId,
        _mute_source: DemuxId,
        _mute_target: DemuxId,
    ) {
        // ignore
    }
}

impl VideoSink for Observer {
    fn on_video_frame(&self, demux_id: DemuxId, frame: VideoFrame) {
        self.last_frame_metadata_by_demux_id
            .lock()
            .unwrap()
            .insert(demux_id, frame.metadata());
    }

    fn box_clone(&self) -> Box<dyn VideoSink> {
        Box::new(self.clone())
    }
}

struct Log;

static LOG: Log = Log;

impl log::Log for Log {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!("{} - {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let url = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("https://sfu.voip.signal.org");
    let membership_proof = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("757365725f6964:67726f75705f6964:1:"); // Hex of "user_id:group_id:timestamp:" with empty MAC
    let hkdf_extra_info = vec![1, 2, 3];

    log::set_logger(&LOG).expect("set logger");
    log::set_max_level(log::LevelFilter::Info);
    ringrtc::webrtc::logging::set_logger(log::LevelFilter::Info);

    let group_id = b"Test Group".to_vec();
    let http_client = sim_http::HttpClient::start();
    let sfu_client = Box::new(HttpSfuClient::new(
        Box::new(http_client),
        url.to_string(),
        None,
        None,
        None,
        hkdf_extra_info,
    ));
    let observer = Observer::default();
    let peer_connection_factory =
        PeerConnectionFactory::new(&peer_connection_factory::AudioConfig::default(), false)
            .unwrap();
    let outgoing_audio_track = peer_connection_factory
        .create_outgoing_audio_track()
        .unwrap();
    let outgoing_video_source = peer_connection_factory
        .create_outgoing_video_source()
        .unwrap();
    let outgoing_video_track = peer_connection_factory
        .create_outgoing_video_track(&outgoing_video_source)
        .unwrap();
    let busy = Arc::new(CallMutex::new(false, "busy"));
    let self_uuid = Arc::new(CallMutex::new(None, "self_uuid"));
    let obfuscated_resolver = ObfuscatedResolver::new(Arc::new(MemberMap::new(&[])), None);

    let client = group_call::Client::start(group_call::ClientStartParams {
        group_id,
        client_id: 1,
        kind: group_call::GroupCallKind::SignalGroup,
        sfu_client,
        proxy_info: None,
        obfuscated_resolver,
        observer: Box::new(observer.clone()),
        busy,
        self_uuid,
        peer_connection_factory: None,
        outgoing_audio_track,
        outgoing_video_track: Some(outgoing_video_track.clone()),
        incoming_video_sink: Some(Box::new(observer.clone())),
        ring_id: None,
        audio_levels_interval: None,
    })
    .unwrap();

    let send_rate_override = DataRate::from_mbps(10);
    client.override_send_rates(SendRates {
        min: Some(send_rate_override),
        start: Some(send_rate_override),
        max: Some(send_rate_override),
    });
    client.set_membership_proof(membership_proof.as_bytes().to_vec());
    client.connect();
    client.join();
    outgoing_video_track.set_enabled(true);

    std::thread::spawn(move || {
        for index in 0u64.. {
            let width = 1280;
            let height = 720;
            let rgba_data: Vec<u8> = (0..(width * height * 4))
                .map(|i: u32| i.wrapping_add(index as u32) as u8)
                .collect();
            outgoing_video_source.push_frame(VideoFrame::copy_from_slice(
                width,
                height,
                VideoPixelFormat::Rgba,
                &rgba_data,
            ));
            std::thread::sleep(std::time::Duration::from_secs_f32(1.0 / 30.0));
        }
    });

    let mut request_big_next_time = true;
    std::thread::sleep(std::time::Duration::from_secs(1));
    loop {
        let (width, height) = if request_big_next_time {
            (10000, 10000)
        } else {
            (1, 1)
        };
        request_big_next_time = !request_big_next_time;
        let requests = observer
            .remote_devices
            .lock()
            .unwrap()
            .iter()
            .map(|remote| {
                group_call::VideoRequest {
                    demux_id: remote.demux_id,
                    width,
                    height,
                    framerate: None, // Unrestrained
                }
            })
            .collect();
        info!("Request video of size {}x{}", width, height);
        info!("Requests: {:?}", requests);
        info!(
            "Current videos: {}",
            observer
                .last_frame_metadata_by_demux_id
                .lock()
                .unwrap()
                .len()
        );
        for (demux_id, metadata) in observer
            .last_frame_metadata_by_demux_id
            .lock()
            .unwrap()
            .iter()
        {
            info!("  {} {:?}", demux_id, metadata);
        }
        client.request_video(requests, height);
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}
