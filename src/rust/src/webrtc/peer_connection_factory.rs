//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! WebRTC Peer Connection

#[cfg(all(not(feature = "sim"), feature = "native"))]
use std::ffi::c_void;
#[cfg(feature = "native")]
use std::ffi::CStr;
#[cfg(all(not(feature = "sim"), feature = "native"))]
use std::sync::{Arc, Mutex};
use std::{ffi::CString, os::raw::c_char};

use anyhow::anyhow;
pub use pcf::{RffiPeerConnectionFactoryInterface, RffiPeerConnectionFactoryOwner};

#[cfg(all(not(feature = "sim"), feature = "native"))]
use crate::webrtc::audio_device_module::AudioDeviceModule;
#[cfg(all(not(feature = "sim"), feature = "native"))]
use crate::webrtc::ffi::audio_device_module::{decrement_adm_ref_count, AUDIO_DEVICE_CBS_PTR};
#[cfg(not(feature = "sim"))]
use crate::webrtc::ffi::peer_connection_factory as pcf;
#[cfg(feature = "injectable_network")]
use crate::webrtc::injectable_network::InjectableNetwork;
#[cfg(feature = "sim")]
use crate::webrtc::sim::peer_connection_factory as pcf;
use crate::{
    common::Result,
    error::RingRtcError,
    webrtc,
    webrtc::{
        media::{AudioTrack, VideoSource, VideoTrack},
        peer_connection::PeerConnection,
        peer_connection_observer::{PeerConnectionObserver, PeerConnectionObserverTrait},
    },
};

#[cfg(feature = "native")]
const ADM_MAX_DEVICE_NAME_SIZE: usize = 128;
#[cfg(feature = "native")]
const ADM_MAX_DEVICE_UUID_SIZE: usize = 128;

#[repr(C)]
pub struct RffiIceServer {
    pub username: webrtc::ptr::Borrowed<c_char>,
    pub password: webrtc::ptr::Borrowed<c_char>,
    pub hostname: webrtc::ptr::Borrowed<c_char>,
    pub urls: webrtc::ptr::Borrowed<webrtc::ptr::Borrowed<c_char>>,
    pub urls_size: usize,
}

#[repr(u8)]
pub enum RffiPeerConnectionKind {
    Direct,
    Relayed,
    GroupCall,
}

#[derive(Clone, Debug, Default)]
pub struct IceServer {
    username: CString,
    password: CString,
    hostname: CString,
    // To own the strings
    _urls: Vec<CString>,
    // To hand the strings to C
    url_ptrs: Vec<webrtc::ptr::Borrowed<c_char>>,
}

unsafe impl Send for IceServer {}
unsafe impl Sync for IceServer {}

impl IceServer {
    pub fn new(username: String, password: String, hostname: String, urls_in: Vec<String>) -> Self {
        let mut urls = Vec::new();
        for url in urls_in {
            urls.push(CString::new(url).expect("CString of URL"));
        }
        let url_ptrs = urls
            .iter()
            .map(|s| webrtc::ptr::Borrowed::from_ptr(s.as_ptr()))
            .collect();
        Self {
            username: CString::new(username).expect("CString of username"),
            password: CString::new(password).expect("CString of password"),
            hostname: CString::new(hostname).expect("CString of hostname"),
            _urls: urls,
            url_ptrs,
        }
    }

    pub fn none() -> Self {
        // In the FFI C++, no urls means no IceServer is added
        Self::new(
            "".to_string(), // username
            "".to_string(), // password
            "".to_string(), // hostname
            vec![],         // urls
        )
    }

    pub fn rffi(&self) -> RffiIceServer {
        RffiIceServer {
            username: webrtc::ptr::Borrowed::from_ptr(self.username.as_ptr()),
            password: webrtc::ptr::Borrowed::from_ptr(self.password.as_ptr()),
            hostname: webrtc::ptr::Borrowed::from_ptr(self.hostname.as_ptr()),
            urls: webrtc::ptr::Borrowed::from_ptr(self.url_ptrs.as_ptr()),
            urls_size: self.url_ptrs.len(),
        }
    }
}

#[repr(C)]
pub struct RffiIceServers {
    servers: webrtc::ptr::Borrowed<RffiIceServer>,
    servers_size: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RffiProxyType {
    #[default]
    None,
    Https,
    Socks5,
    Unknown,
}

impl RffiProxyType {
    pub fn from_u8(u: u8) -> Self {
        match u {
            0 => RffiProxyType::None,
            1 => RffiProxyType::Https,
            2 => RffiProxyType::Socks5,
            _ => RffiProxyType::Unknown,
        }
    }
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct RffiProxyInfo {
    pub proxy_type: RffiProxyType,
    pub hostname: webrtc::ptr::Borrowed<c_char>,
    pub username: webrtc::ptr::Borrowed<c_char>,
    pub password: webrtc::ptr::Borrowed<c_char>,
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct ProxyInfo {
    proxy_type: RffiProxyType,
    hostname: CString,
    password: CString,
    username: CString,
    port: u16,
}

impl ProxyInfo {
    pub fn new(
        proxy_type: RffiProxyType,
        hostname: &str,
        password: &str,
        username: &str,
        port: u16,
    ) -> Self {
        Self {
            proxy_type,
            hostname: CString::new(hostname).unwrap(),
            password: CString::new(password).unwrap(),
            username: CString::new(username).unwrap(),
            port,
        }
    }

    pub fn none() -> Self {
        Self {
            proxy_type: RffiProxyType::None,
            hostname: CString::new("").unwrap(),
            username: CString::new("").unwrap(),
            password: CString::new("").unwrap(),
            port: 0,
        }
    }

    pub fn rffi(&self) -> RffiProxyInfo {
        RffiProxyInfo {
            proxy_type: self.proxy_type,
            hostname: webrtc::ptr::Borrowed::from_ptr(self.hostname.as_ptr()),
            username: webrtc::ptr::Borrowed::from_ptr(self.username.as_ptr()),
            password: webrtc::ptr::Borrowed::from_ptr(self.password.as_ptr()),
            port: self.port,
        }
    }
}

/// Describes an audio input or output device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// Name of the device
    pub name: String,
    /// Unique ID - truly unique on Windows, best effort on other platforms.
    pub unique_id: String,
    /// If the name requires translation, the translated string identifier.
    pub i18n_key: String,
}

/// Stays in sync with RffiAudioDeviceModuleType in peer_connection_factory.h.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RffiAudioDeviceModuleType {
    // 0 used to represent webrtc's default ADM, but this is no longer used.
    /// Use a file-based ADM for testing and simulation.
    File = 1,
    /// Use RingRTC's ADM implementation.
    #[default]
    RingRtc,
}

/// Stays in sync with RffiAudioConfig in peer_connection_factory.h.
#[repr(C)]
pub struct RffiAudioConfig {
    pub audio_device_module_type: RffiAudioDeviceModuleType,
    pub input_file: webrtc::ptr::Borrowed<c_char>,
    pub output_file: webrtc::ptr::Borrowed<c_char>,
    pub high_pass_filter_enabled: bool,
    pub aec_enabled: bool,
    pub ns_enabled: bool,
    pub agc_enabled: bool,
    #[cfg(all(not(feature = "sim"), feature = "native"))]
    pub adm_borrowed: webrtc::ptr::Borrowed<c_void>,
    #[cfg(all(not(feature = "sim"), feature = "native"))]
    pub rust_audio_device_callbacks: webrtc::ptr::Borrowed<c_void>,
    #[cfg(all(not(feature = "sim"), feature = "native"))]
    pub free_adm_cb: unsafe extern "C" fn(webrtc::ptr::Borrowed<c_void>),
}
pub struct RffiAudioConfigWrapper {
    rffi: RffiAudioConfig,
    #[cfg(all(not(feature = "sim"), feature = "native"))]
    adm: Option<Arc<Mutex<AudioDeviceModule>>>,
}

#[derive(Clone, Debug)]
pub struct FileBasedAdmConfig {
    pub input_file: CString,
    pub output_file: CString,
}

#[derive(Clone, Debug)]
pub struct AudioConfig {
    pub audio_device_module_type: RffiAudioDeviceModuleType,
    pub file_based_adm_config: Option<FileBasedAdmConfig>,
    pub high_pass_filter_enabled: bool,
    pub aec_enabled: bool,
    pub ns_enabled: bool,
    pub agc_enabled: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            audio_device_module_type: Default::default(),
            file_based_adm_config: None,
            high_pass_filter_enabled: true,
            aec_enabled: true,
            ns_enabled: true,
            agc_enabled: true,
        }
    }
}

// An observer trait that receives notifications whenever the input or output
// devices change.
// These callbacks should run "quickly", as they'll be run from the cubeb worker
// thread, and any delays will block playout_delay calls, which happen
// constantly during calls (short blocking is OK, as is passing the
// event to another thread; calling a client directly is not).
// Currently only applicable on Desktop
pub trait AudioDeviceObserver: Send + std::fmt::Debug {
    fn output_changed(&self, devices: Vec<Option<AudioDevice>>);
    fn input_changed(&self, devices: Vec<Option<AudioDevice>>);
}

impl AudioConfig {
    // Return both the RffiAudioConfig as well as the name of the cubeb backend
    // in use, if any.
    fn rffi(
        &self,
        #[allow(unused_mut, unused_variables)] // iOS and Android won't use this; that's fine.
        mut audio_device_observer: Option<Box<dyn AudioDeviceObserver>>,
    ) -> Result<RffiAudioConfigWrapper> {
        let (input_file, output_file) =
            if self.audio_device_module_type == RffiAudioDeviceModuleType::File {
                if let Some(file_based_adm_config) = &self.file_based_adm_config {
                    (
                        file_based_adm_config.input_file.as_ptr(),
                        file_based_adm_config.output_file.as_ptr(),
                    )
                } else {
                    return Err(anyhow!("no files specified for the file-based ADM!"));
                }
            } else {
                (std::ptr::null(), std::ptr::null())
            };

        #[cfg(all(not(feature = "sim"), feature = "native"))]
        let (adm_borrowed, adm_arc) =
            if self.audio_device_module_type == RffiAudioDeviceModuleType::RingRtc {
                match AudioDeviceModule::new() {
                    Ok(mut adm) => {
                        if let Some(observer) = audio_device_observer.take() {
                            adm.register_audio_device_callback(observer)?;
                        }
                        let adm_arc = Arc::new(Mutex::new(adm));
                        (
                            // This will need to be explicitly destroyed by the
                            // C++ layer by calling decrement_adm_ref_count to
                            // turn it back into an Arc.
                            // We use into_raw(...clone()) here to ensure that
                            // the ADM stays alive until the C++ layer is done
                            // using it.
                            webrtc::ptr::Borrowed::from_ptr(
                                Arc::<Mutex<AudioDeviceModule>>::into_raw(adm_arc.clone()),
                            )
                            .to_void(),
                            Some(adm_arc),
                        )
                    }
                    Err(e) => {
                        error!("Failed to initialize adm: {}", e);
                        (webrtc::ptr::Borrowed::null(), None)
                    }
                }
            } else {
                (webrtc::ptr::Borrowed::null(), None)
            };

        Ok(RffiAudioConfigWrapper {
            rffi: RffiAudioConfig {
                audio_device_module_type: self.audio_device_module_type,
                input_file: webrtc::ptr::Borrowed::from_ptr(input_file),
                output_file: webrtc::ptr::Borrowed::from_ptr(output_file),
                high_pass_filter_enabled: self.high_pass_filter_enabled,
                aec_enabled: self.aec_enabled,
                ns_enabled: self.ns_enabled,
                agc_enabled: self.agc_enabled,
                #[cfg(all(not(feature = "sim"), feature = "native"))]
                adm_borrowed,
                #[cfg(all(not(feature = "sim"), feature = "native"))]
                rust_audio_device_callbacks: webrtc::ptr::Borrowed::from_ptr(AUDIO_DEVICE_CBS_PTR)
                    .to_void(),
                #[cfg(all(not(feature = "sim"), feature = "native"))]
                free_adm_cb: decrement_adm_ref_count,
            },
            #[cfg(all(not(feature = "sim"), feature = "native"))]
            adm: adm_arc,
        })
    }
}

/// Stays in sync with RffiAudioJitterBufferConfig in peer_connection_factory.h.
#[repr(C)]
pub struct RffiAudioJitterBufferConfig {
    pub max_packets: i32,
    pub min_delay_ms: i32,
    pub max_target_delay_ms: i32,
    pub fast_accelerate: bool,
}

#[derive(Clone, Debug)]
pub struct AudioJitterBufferConfig {
    pub max_packets: i32,
    pub min_delay_ms: i32,
    pub max_target_delay_ms: i32,
    pub fast_accelerate: bool,
}

impl Default for AudioJitterBufferConfig {
    fn default() -> Self {
        Self {
            max_packets: 50,
            min_delay_ms: 0,
            max_target_delay_ms: 500,
            fast_accelerate: false,
        }
    }
}

impl AudioJitterBufferConfig {
    fn rffi(&self) -> RffiAudioJitterBufferConfig {
        RffiAudioJitterBufferConfig {
            max_packets: self.max_packets,
            min_delay_ms: self.min_delay_ms,
            max_target_delay_ms: self.max_target_delay_ms,
            fast_accelerate: self.fast_accelerate,
        }
    }
}

#[cfg(feature = "native")]
#[derive(Clone, Debug, Default)]
pub struct DeviceCounts {
    playout: Option<u16>,
    recording: Option<u16>,
}

/// Rust wrapper around WebRTC C++ PeerConnectionFactory object.
#[derive(Clone, Debug)]
pub struct PeerConnectionFactory {
    rffi: webrtc::Arc<RffiPeerConnectionFactoryOwner>,
    #[cfg(feature = "native")]
    device_counts: DeviceCounts,
    // Hold this so we run `drop` on it on shutdown
    #[cfg(all(not(feature = "sim"), feature = "native"))]
    adm: Option<Arc<Mutex<AudioDeviceModule>>>,
}

impl PeerConnectionFactory {
    /// Create a new Rust PeerConnectionFactory object from a WebRTC C++
    /// PeerConnectionFactory object.
    pub fn new(
        audio_config: &AudioConfig,
        use_injectable_network: bool,
        audio_device_observer: Option<Box<dyn AudioDeviceObserver>>,
    ) -> Result<Self> {
        debug!("PeerConnectionFactory::new()");

        let audio_config_rffi = audio_config.rffi(audio_device_observer)?;

        let rffi = unsafe {
            webrtc::Arc::from_owned(pcf::Rust_createPeerConnectionFactory(
                webrtc::ptr::Borrowed::from_ptr(&audio_config_rffi.rffi),
                use_injectable_network,
            ))
        };
        if rffi.is_null() {
            return Err(RingRtcError::CreatePeerConnectionFactory.into());
        }
        Ok(Self {
            rffi,
            #[cfg(feature = "native")]
            device_counts: Default::default(),
            #[cfg(all(not(feature = "sim"), feature = "native"))]
            adm: audio_config_rffi.adm,
        })
    }

    pub fn rffi(&self) -> &webrtc::Arc<RffiPeerConnectionFactoryOwner> {
        &self.rffi
    }

    /// Wrap an existing C++ PeerConnectionFactory (not a PeerConnectionFactoryOwner).
    ///
    /// # Safety
    ///
    /// `native` must point to a C++ PeerConnectionFactory.
    pub unsafe fn from_native_factory(
        native: webrtc::Arc<RffiPeerConnectionFactoryInterface>,
    ) -> Self {
        let rffi = unsafe {
            webrtc::Arc::from_owned(pcf::Rust_createPeerConnectionFactoryWrapper(
                native.as_borrowed(),
            ))
        };
        Self {
            rffi,
            #[cfg(feature = "native")]
            device_counts: Default::default(),
            #[cfg(all(not(feature = "sim"), feature = "native"))]
            adm: None,
        }
    }

    #[cfg(feature = "injectable_network")]
    pub fn injectable_network(&self) -> Option<InjectableNetwork> {
        let rffi = unsafe { pcf::Rust_getInjectableNetwork(self.rffi.as_borrowed()) };
        if rffi.is_null() {
            return None;
        }
        Some(InjectableNetwork::new(rffi, self.rffi.clone()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_peer_connection<T: PeerConnectionObserverTrait>(
        &self,
        pc_observer: PeerConnectionObserver<T>,
        kind: RffiPeerConnectionKind,
        audio_jitter_buffer_config: &AudioJitterBufferConfig,
        audio_rtcp_report_interval_ms: i32,
        ice_servers: &[IceServer],
        proxy_info: &ProxyInfo,
        outgoing_audio_track: AudioTrack,
        outgoing_video_track: Option<VideoTrack>,
    ) -> Result<PeerConnection> {
        debug!(
            "PeerConnectionFactory::create_peer_connection() {:?}",
            self.rffi
        );
        // Unlike on Android (see call_manager::create_peer_connection)
        // and iOS (see IosPlatform::create_connection),
        // the RffiPeerConnectionObserver is *not* passed as owned
        // by Rust_createPeerConnection, so we need to keep it alive
        // for as long as the native PeerConnection is alive.
        // we do this by passing a webrtc::ptr::Unique<RffiPeerConnectionObserver> to
        // the Rust-level PeerConnection and let it own it.
        let pc_observer_rffi = pc_observer.into_rffi();
        let servers: Vec<RffiIceServer> = ice_servers.iter().map(|s| s.rffi()).collect();
        let rffi_ice_servers = RffiIceServers {
            servers: webrtc::ptr::Borrowed::from_ptr(servers.as_ptr()),
            servers_size: servers.len(),
        };
        let rffi_proxy_info = proxy_info.rffi();

        let rffi = webrtc::Arc::from_owned(unsafe {
            pcf::Rust_createPeerConnection(
                self.rffi.as_borrowed(),
                pc_observer_rffi.borrow(),
                kind,
                webrtc::ptr::Borrowed::from_ptr(&audio_jitter_buffer_config.rffi()),
                audio_rtcp_report_interval_ms,
                webrtc::ptr::Borrowed::from_ptr(&rffi_ice_servers),
                webrtc::ptr::Borrowed::from_ptr(&rffi_proxy_info),
                outgoing_audio_track.rffi().as_borrowed(),
                outgoing_video_track
                    .map_or_else(webrtc::ptr::BorrowedRc::null, |outgoing_video_track| {
                        outgoing_video_track.rffi().as_borrowed()
                    }),
            )
        });
        debug!(
            "PeerConnectionFactory::create_peer_connection() finished: {:?}",
            rffi
        );
        if rffi.is_null() {
            return Err(RingRtcError::CreatePeerConnection.into());
        }
        Ok(PeerConnection::new(
            rffi,
            Some(pc_observer_rffi),
            Some(self.rffi.clone()),
        ))
    }

    pub fn create_outgoing_audio_track(&self) -> Result<AudioTrack> {
        debug!("PeerConnectionFactory::create_outgoing_audio_track()");
        let rffi =
            webrtc::Arc::from_owned(unsafe { pcf::Rust_createAudioTrack(self.rffi.as_borrowed()) });
        if rffi.is_null() {
            return Err(RingRtcError::CreateAudioTrack.into());
        }
        Ok(AudioTrack::new(rffi, Some(self.rffi.clone())))
    }

    pub fn create_outgoing_video_source(&self) -> Result<VideoSource> {
        debug!("PeerConnectionFactory::create_outgoing_video_source()");
        let rffi = webrtc::Arc::from_owned(unsafe { pcf::Rust_createVideoSource() });
        if rffi.is_null() {
            return Err(RingRtcError::CreateVideoSource.into());
        }
        Ok(VideoSource::new(rffi))
    }

    // We take ownership of the VideoSource because Rust_createVideoTrack takes ownership
    // of one takes ownership of one ref count to the source.
    pub fn create_outgoing_video_track(
        &self,
        outgoing_video_source: &VideoSource,
    ) -> Result<VideoTrack> {
        debug!("PeerConnectionFactory::create_outgoing_video_track()");
        let rffi = webrtc::Arc::from_owned(unsafe {
            pcf::Rust_createVideoTrack(
                self.rffi.as_borrowed(),
                outgoing_video_source.rffi().as_borrowed(),
            )
        });
        if rffi.is_null() {
            return Err(RingRtcError::CreateVideoTrack.into());
        }
        Ok(VideoTrack::new(rffi, Some(self.rffi.clone())))
    }

    #[cfg(feature = "native")]
    fn get_audio_playout_device(&self, index: u16) -> Result<AudioDevice> {
        let mut name_buf = [0; ADM_MAX_DEVICE_NAME_SIZE];
        let mut unique_id_buf = [0; ADM_MAX_DEVICE_UUID_SIZE];
        let rc = unsafe {
            pcf::Rust_getAudioPlayoutDeviceName(
                self.rffi.as_borrowed(),
                index,
                name_buf.as_mut_ptr(),
                unique_id_buf.as_mut_ptr(),
            )
        };
        if rc != 0 {
            error!("getAudioPlayoutDeviceName({}) failed: {}", index, rc);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        // SAFETY: the buffer pointers will be valid until the end of the scope,
        // and they should contain valid C strings if the return code indicated success.
        let name = unsafe { CStr::from_ptr(name_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let unique_id = unsafe { CStr::from_ptr(unique_id_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Ok(AudioDevice {
            name,
            unique_id,
            i18n_key: "".to_string(),
        })
    }

    #[cfg(feature = "native")]
    pub fn get_audio_playout_devices(&mut self) -> Result<Vec<AudioDevice>> {
        let device_count = unsafe { pcf::Rust_getAudioPlayoutDevices(self.rffi.as_borrowed()) };
        if device_count < 0 {
            error!("getAudioPlayoutDevices() returned {}", device_count);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        let device_count = device_count as u16;
        let mut devices = Vec::<AudioDevice>::new();

        #[cfg(target_os = "windows")]
        // If there is at least one real device, add slots for the "default" and
        // "default communications" device. When setting, the ADM already has them,
        // but doesn't include them in the count.
        let device_count = if device_count > 0 {
            device_count + 2
        } else {
            0
        };

        if self.device_counts.playout != Some(device_count) {
            info!(
                "PeerConnectionFactory::get_audio_playout_devices(): device_count: {}",
                device_count
            );
            self.device_counts.playout = Some(device_count);
        }

        for i in 0..device_count {
            match self.get_audio_playout_device(i) {
                Ok(dev) => devices.push(dev),
                Err(fail) => {
                    error!("getAudioPlayoutDevice({}) failed: {}", i, fail);
                    return Err(fail);
                }
            }
        }
        // For devices missing unique_id, populate them with name + index
        for i in 0..devices.len() {
            if devices[i].unique_id.is_empty() {
                let same_name_count = devices[..i]
                    .iter()
                    .filter(|d| d.name == devices[i].name)
                    .count() as u16;
                devices[i].unique_id = format!("{}-{}", devices[i].name, same_name_count);
            }
        }

        #[cfg(target_os = "windows")]
        if devices.len() > 1 {
            // Swap the first two devices, so that the "default communications" device
            // is first and the "default" device is second. The UI treats the first
            // index as the default, which for VoIP we prefer communications devices.
            devices.swap(0, 1);

            // Also, give both of those artificial slots unique ids so that
            // the UI can manage them correctly.
            devices[0].unique_id.push_str("-0");
            devices[1].unique_id.push_str("-1");
        }

        Ok(devices)
    }

    #[cfg(feature = "native")]
    pub fn set_audio_playout_device(&mut self, index: u16) -> Result<()> {
        #[cfg(target_os = "windows")]
        // Swap the first two devices back to ordinal if either are selected.
        let index = match index {
            0 => 1,
            1 => 0,
            _ => index,
        };

        info!("PeerConnectionFactory::set_audio_playout_device({})", index);

        let ok = unsafe { pcf::Rust_setAudioPlayoutDevice(self.rffi.as_borrowed(), index) };
        if ok {
            Ok(())
        } else {
            error!("setAudioPlayoutDevice({}) failed", index);
            Err(RingRtcError::SetAudioDevice.into())
        }
    }

    #[cfg(feature = "native")]
    fn get_audio_recording_device(&self, index: u16) -> Result<AudioDevice> {
        let mut name_buf = [0; ADM_MAX_DEVICE_NAME_SIZE];
        let mut unique_id_buf = [0; ADM_MAX_DEVICE_UUID_SIZE];
        let rc = unsafe {
            pcf::Rust_getAudioRecordingDeviceName(
                self.rffi.as_borrowed(),
                index,
                name_buf.as_mut_ptr(),
                unique_id_buf.as_mut_ptr(),
            )
        };
        if rc != 0 {
            error!("getAudioRecordingDeviceName({}) failed: {}", index, rc);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        // SAFETY: the buffer pointers will be valid until the end of the scope,
        // and they should contain valid C strings if the return code indicated success.
        let name = unsafe { CStr::from_ptr(name_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let unique_id = unsafe { CStr::from_ptr(unique_id_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Ok(AudioDevice {
            name,
            unique_id,
            i18n_key: "".to_string(),
        })
    }

    #[cfg(feature = "native")]
    pub fn get_audio_recording_devices(&mut self) -> Result<Vec<AudioDevice>> {
        let device_count = unsafe { pcf::Rust_getAudioRecordingDevices(self.rffi.as_borrowed()) };
        if device_count < 0 {
            error!("getAudioRecordingDevices() returned {}", device_count);
            return Err(RingRtcError::QueryAudioDevices.into());
        }
        let device_count = device_count as u16;
        let mut devices = Vec::<AudioDevice>::new();

        #[cfg(target_os = "windows")]
        // If there is at least one real device, add slots for the "default" and
        // "default communications" device. When setting, the ADM already has them,
        // but doesn't include them in the count.
        let device_count = if device_count > 0 {
            device_count + 2
        } else {
            0
        };

        if self.device_counts.recording != Some(device_count) {
            info!(
                "PeerConnectionFactory::get_audio_recording_devices(): device_count: {}",
                device_count
            );
            self.device_counts.recording = Some(device_count);
        }

        for i in 0..device_count {
            match self.get_audio_recording_device(i) {
                Ok(dev) => devices.push(dev),
                Err(fail) => {
                    error!("getAudioRecordingDevice({}) failed: {}", i, fail);
                    return Err(fail);
                }
            }
        }
        // For devices missing unique_id, populate them with name + index
        for i in 0..devices.len() {
            if devices[i].unique_id.is_empty() {
                let same_name_count = devices[..i]
                    .iter()
                    .filter(|d| d.name == devices[i].name)
                    .count() as u16;
                devices[i].unique_id = format!("{}-{}", devices[i].name, same_name_count);
            }
        }

        #[cfg(target_os = "windows")]
        if devices.len() > 1 {
            // Swap the first two devices, so that the "default communications" device
            // is first and the "default" device is second. The UI treats the first
            // index as the default, which for VoIP we prefer communications devices.
            devices.swap(0, 1);

            // Also, give both of those artificial slots unique ids so that
            // the UI can manage them correctly.
            devices[0].unique_id.push_str("-0");
            devices[1].unique_id.push_str("-1");
        }

        Ok(devices)
    }

    #[cfg(feature = "native")]
    pub fn set_audio_recording_device(&mut self, index: u16) -> Result<()> {
        #[cfg(target_os = "windows")]
        // Swap the first two devices back to ordinal if either are selected.
        let index = match index {
            0 => 1,
            1 => 0,
            _ => index,
        };

        info!(
            "PeerConnectionFactory::set_audio_recording_device({})",
            index
        );

        let ok = unsafe { pcf::Rust_setAudioRecordingDevice(self.rffi.as_borrowed(), index) };
        if ok {
            Ok(())
        } else {
            error!("setAudioRecordingDevice({}) failed", index);
            Err(RingRtcError::SetAudioDevice.into())
        }
    }

    #[cfg(all(not(feature = "sim"), feature = "native"))]
    pub fn audio_backend(&self) -> Option<String> {
        self.adm
            .as_ref()
            .and_then(|adm| adm.lock().ok())
            .map(|adm| adm.backend_name())
    }
}
