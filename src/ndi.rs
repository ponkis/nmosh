use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    path::PathBuf,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use libloading::Library;

#[derive(Clone, Debug)]
pub struct NdiFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub struct NdiInput {
    latest: Arc<Mutex<Option<NdiFrame>>>,
    status: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl NdiInput {
    pub fn start(source_name: Option<String>, library_path: Option<PathBuf>) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let status = Arc::new(Mutex::new("starting".to_string()));
        let stop = Arc::new(AtomicBool::new(false));

        let thread_latest = Arc::clone(&latest);
        let thread_status = Arc::clone(&status);
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            run_ndi_worker(
                source_name,
                library_path,
                thread_latest,
                thread_status,
                thread_stop,
            );
        });

        Self {
            latest,
            status,
            stop,
            handle: Some(handle),
        }
    }

    pub fn take_latest_frame(&self) -> Option<NdiFrame> {
        self.latest.lock().ok()?.take()
    }

    pub fn status(&self) -> String {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_else(|_| "status unavailable".to_string())
    }
}

impl Drop for NdiInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

type NdiFindInstance = *mut c_void;
type NdiRecvInstance = *mut c_void;

const NDI_FRAME_TYPE_NONE: c_int = 0;
const NDI_FRAME_TYPE_VIDEO: c_int = 1;
const NDI_FRAME_TYPE_ERROR: c_int = 4;
const NDI_FRAME_TYPE_STATUS_CHANGE: c_int = 100;
const NDI_RECV_COLOR_FORMAT_BGRX_BGRA: c_int = 2;
const NDI_RECV_BANDWIDTH_HIGHEST: c_int = 100;

#[repr(C)]
#[derive(Clone, Copy)]
struct NdiSource {
    p_ndi_name: *const c_char,
    p_url_address: *const c_char,
}

#[repr(C)]
struct NdiFindCreate {
    show_local_sources: bool,
    p_groups: *const c_char,
    p_extra_ips: *const c_char,
}

#[repr(C)]
struct NdiRecvCreateV3 {
    source_to_connect_to: NdiSource,
    color_format: c_int,
    bandwidth: c_int,
    allow_video_fields: bool,
    p_ndi_recv_name: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NdiVideoFrameV2 {
    xres: c_int,
    yres: c_int,
    four_cc: c_int,
    frame_rate_n: c_int,
    frame_rate_d: c_int,
    picture_aspect_ratio: f32,
    frame_format_type: c_int,
    timecode: i64,
    p_data: *mut u8,
    line_stride_in_bytes: c_int,
    p_metadata: *const c_char,
    timestamp: i64,
}

impl Default for NdiVideoFrameV2 {
    fn default() -> Self {
        Self {
            xres: 0,
            yres: 0,
            four_cc: 0,
            frame_rate_n: 0,
            frame_rate_d: 0,
            picture_aspect_ratio: 0.0,
            frame_format_type: 0,
            timecode: 0,
            p_data: ptr::null_mut(),
            line_stride_in_bytes: 0,
            p_metadata: ptr::null(),
            timestamp: 0,
        }
    }
}

struct NdiApi {
    _library: Library,
    initialize: unsafe extern "C" fn() -> bool,
    destroy: unsafe extern "C" fn(),
    version: unsafe extern "C" fn() -> *const c_char,
    find_create_v2: unsafe extern "C" fn(*const NdiFindCreate) -> NdiFindInstance,
    find_destroy: unsafe extern "C" fn(NdiFindInstance),
    find_wait_for_sources: unsafe extern "C" fn(NdiFindInstance, u32) -> bool,
    find_get_current_sources: unsafe extern "C" fn(NdiFindInstance, *mut u32) -> *const NdiSource,
    recv_create_v3: unsafe extern "C" fn(*const NdiRecvCreateV3) -> NdiRecvInstance,
    recv_destroy: unsafe extern "C" fn(NdiRecvInstance),
    recv_capture_v2: unsafe extern "C" fn(
        NdiRecvInstance,
        *mut NdiVideoFrameV2,
        *mut c_void,
        *mut c_void,
        u32,
    ) -> c_int,
    recv_free_video_v2: unsafe extern "C" fn(NdiRecvInstance, *const NdiVideoFrameV2),
}

impl NdiApi {
    unsafe fn load(preferred_path: Option<PathBuf>) -> Result<Self, String> {
        let candidates = ndi_library_candidates(preferred_path);
        let mut load_errors = Vec::new();

        for candidate in candidates {
            match Library::new(&candidate.path) {
                Ok(library) => {
                    log::info!("Loaded NDI runtime from {}", candidate.label);
                    return Self::from_library(library);
                }
                Err(error) => load_errors.push(format!("{}: {error}", candidate.label)),
            }
        }

        Err(format!(
            "could not load NDI runtime. Tried: {}",
            load_errors.join(", ")
        ))
    }

    unsafe fn from_library(library: Library) -> Result<Self, String> {
        Ok(Self {
            initialize: symbol(&library, b"NDIlib_initialize\0")?,
            destroy: symbol(&library, b"NDIlib_destroy\0")?,
            version: symbol(&library, b"NDIlib_version\0")?,
            find_create_v2: symbol(&library, b"NDIlib_find_create_v2\0")?,
            find_destroy: symbol(&library, b"NDIlib_find_destroy\0")?,
            find_wait_for_sources: symbol(&library, b"NDIlib_find_wait_for_sources\0")?,
            find_get_current_sources: symbol(&library, b"NDIlib_find_get_current_sources\0")?,
            recv_create_v3: symbol(&library, b"NDIlib_recv_create_v3\0")?,
            recv_destroy: symbol(&library, b"NDIlib_recv_destroy\0")?,
            recv_capture_v2: symbol(&library, b"NDIlib_recv_capture_v2\0")?,
            recv_free_video_v2: symbol(&library, b"NDIlib_recv_free_video_v2\0")?,
            _library: library,
        })
    }
}

unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
    library
        .get::<T>(name)
        .map(|symbol| *symbol)
        .map_err(|error| format!("missing symbol {}: {error}", String::from_utf8_lossy(name)))
}

struct LibraryCandidate {
    path: PathBuf,
    label: String,
}

fn ndi_library_candidates(preferred_path: Option<PathBuf>) -> Vec<LibraryCandidate> {
    let mut candidates = Vec::new();

    if let Some(path) = preferred_path {
        push_candidate(&mut candidates, path);
    }

    if let Some(path) = std::env::var_os("NMOSH_NDI_DLL") {
        push_candidate(&mut candidates, PathBuf::from(path));
    }

    if let Some(path) = std::env::var_os("MIDI_NDI_DISTORTER_NDI_DLL") {
        push_candidate(&mut candidates, PathBuf::from(path));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push_candidate(&mut candidates, dir.join(platform_ndi_library_name()));
        }
    }

    push_candidate(&mut candidates, PathBuf::from(platform_ndi_library_name()));

    #[cfg(target_os = "windows")]
    {
        for base in windows_program_dirs() {
            for relative in [
                r"NDI\NDI 6 Runtime\v6\Processing.NDI.Lib.x64.dll",
                r"NDI\NDI 5 Runtime\v5\Processing.NDI.Lib.x64.dll",
                r"NDI\NDI 4 Runtime\v4\Processing.NDI.Lib.x64.dll",
                r"NDI\NDI 6 SDK\Bin\x64\Processing.NDI.Lib.x64.dll",
                r"NDI\NDI 5 SDK\Bin\x64\Processing.NDI.Lib.x64.dll",
                r"NewTek\NDI 6 Runtime\v6\Processing.NDI.Lib.x64.dll",
                r"NewTek\NDI 5 Runtime\v5\Processing.NDI.Lib.x64.dll",
                r"NewTek\NDI 4 Runtime\v4\Processing.NDI.Lib.x64.dll",
            ] {
                push_candidate(&mut candidates, base.join(relative));
            }
        }

        push_candidate(&mut candidates, PathBuf::from("Processing.NDI.Lib.x86.dll"));
    }

    #[cfg(target_os = "macos")]
    {
        push_candidate(
            &mut candidates,
            PathBuf::from("/usr/local/lib/libndi.dylib"),
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for path in [
            "/usr/local/lib/libndi.so.6",
            "/usr/local/lib/libndi.so.5",
            "/usr/local/lib/libndi.so",
            "/usr/lib/libndi.so.6",
            "/usr/lib/libndi.so.5",
            "/usr/lib/libndi.so",
        ] {
            push_candidate(&mut candidates, PathBuf::from(path));
        }
    }

    candidates
}

fn push_candidate(candidates: &mut Vec<LibraryCandidate>, path: PathBuf) {
    let label = path.display().to_string();
    if candidates
        .iter()
        .any(|candidate| candidate.label.eq_ignore_ascii_case(&label))
    {
        return;
    }

    candidates.push(LibraryCandidate { path, label });
}

fn platform_ndi_library_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Processing.NDI.Lib.x64.dll"
    }

    #[cfg(target_os = "macos")]
    {
        "libndi.dylib"
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "libndi.so"
    }
}

#[cfg(target_os = "windows")]
fn windows_program_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(path) = std::env::var_os(var) {
            let path = PathBuf::from(path);
            if !dirs.iter().any(|dir| dir == &path) {
                dirs.push(path);
            }
        }
    }

    dirs
}

fn run_ndi_worker(
    source_name: Option<String>,
    library_path: Option<PathBuf>,
    latest: Arc<Mutex<Option<NdiFrame>>>,
    status: Arc<Mutex<String>>,
    stop: Arc<AtomicBool>,
) {
    let api = match unsafe { NdiApi::load(library_path) } {
        Ok(api) => api,
        Err(error) => {
            set_status(&status, error);
            return;
        }
    };

    unsafe {
        if !(api.initialize)() {
            set_status(&status, "NDI initialization failed".to_string());
            return;
        }

        let version = (api.version)();
        if !version.is_null() {
            let version = CStr::from_ptr(version).to_string_lossy();
            log::info!("Loaded NDI runtime: {version}");
        }

        let source = match wait_for_source(&api, source_name.as_deref(), &status, &stop) {
            Some(source) => source,
            None => {
                (api.destroy)();
                return;
            }
        };

        let recv_name = CString::new("nMosh").expect("static receiver name is valid");
        let create = NdiRecvCreateV3 {
            source_to_connect_to: source.as_ndi_source(),
            color_format: NDI_RECV_COLOR_FORMAT_BGRX_BGRA,
            bandwidth: NDI_RECV_BANDWIDTH_HIGHEST,
            allow_video_fields: false,
            p_ndi_recv_name: recv_name.as_ptr(),
        };

        let receiver = (api.recv_create_v3)(&create);
        if receiver.is_null() {
            set_status(&status, "failed to create NDI receiver".to_string());
            (api.destroy)();
            return;
        }

        set_status(&status, format!("connected: {}", source.name()));

        while !stop.load(Ordering::Relaxed) {
            let mut video = NdiVideoFrameV2::default();
            let frame_type =
                (api.recv_capture_v2)(receiver, &mut video, ptr::null_mut(), ptr::null_mut(), 33);

            match frame_type {
                NDI_FRAME_TYPE_VIDEO => {
                    if let Some(frame) = copy_video_frame(&video) {
                        if let Ok(mut slot) = latest.lock() {
                            *slot = Some(frame);
                        }
                    }
                    (api.recv_free_video_v2)(receiver, &video);
                }
                NDI_FRAME_TYPE_STATUS_CHANGE => {
                    set_status(
                        &status,
                        format!("connected: {} (status changed)", source.name()),
                    );
                }
                NDI_FRAME_TYPE_ERROR => {
                    set_status(&status, "NDI receiver error".to_string());
                    break;
                }
                NDI_FRAME_TYPE_NONE => {}
                _ => {}
            }
        }

        (api.recv_destroy)(receiver);
        (api.destroy)();
    }
}

fn wait_for_source(
    api: &NdiApi,
    preferred_name: Option<&str>,
    status: &Arc<Mutex<String>>,
    stop: &AtomicBool,
) -> Option<OwnedSource> {
    let find_create = NdiFindCreate {
        show_local_sources: true,
        p_groups: ptr::null(),
        p_extra_ips: ptr::null(),
    };

    let finder = unsafe { (api.find_create_v2)(&find_create) };
    if finder.is_null() {
        set_status(status, "failed to create NDI source finder".to_string());
        return None;
    }

    let preferred_lower = preferred_name.map(|name| name.to_ascii_lowercase());
    let mut selected = None;

    while !stop.load(Ordering::Relaxed) {
        let mut count = 0_u32;
        let sources = unsafe { (api.find_get_current_sources)(finder, &mut count) };

        if !sources.is_null() && count > 0 {
            let sources = unsafe { std::slice::from_raw_parts(sources, count as usize) };
            let mut first = None;

            for source in sources {
                let Some(owned) = (unsafe { OwnedSource::from_raw(source) }) else {
                    continue;
                };

                if first.is_none() {
                    first = Some(owned.clone());
                }

                if let Some(preferred) = &preferred_lower {
                    if owned.name().to_ascii_lowercase().contains(preferred) {
                        selected = Some(owned);
                        break;
                    }
                }
            }

            if selected.is_none() && preferred_lower.is_none() {
                selected = first;
            }

            if selected.is_some() {
                break;
            }

            if let Some(preferred) = preferred_name {
                set_status(
                    status,
                    format!("waiting for NDI source matching '{preferred}' ({count} visible)"),
                );
            }
        } else {
            set_status(status, "waiting for NDI sources".to_string());
        }

        unsafe {
            (api.find_wait_for_sources)(finder, 500);
        }
    }

    unsafe {
        (api.find_destroy)(finder);
    }

    selected
}

fn copy_video_frame(video: &NdiVideoFrameV2) -> Option<NdiFrame> {
    if video.xres <= 0
        || video.yres <= 0
        || video.p_data.is_null()
        || video.line_stride_in_bytes <= 0
    {
        return None;
    }

    let width = video.xres as usize;
    let height = video.yres as usize;
    let source_stride = video.line_stride_in_bytes as usize;
    let row_bytes = width.checked_mul(4)?;
    if source_stride < row_bytes {
        return None;
    }

    let source_len = source_stride.checked_mul(height)?;
    let source = unsafe { std::slice::from_raw_parts(video.p_data, source_len) };
    let mut data = vec![0_u8; row_bytes.checked_mul(height)?];

    for row in 0..height {
        let source_offset = row * source_stride;
        let dest_offset = row * row_bytes;
        data[dest_offset..dest_offset + row_bytes]
            .copy_from_slice(&source[source_offset..source_offset + row_bytes]);
    }

    Some(NdiFrame {
        width: width as u32,
        height: height as u32,
        data,
    })
}

#[derive(Clone)]
struct OwnedSource {
    name: CString,
    url: Option<CString>,
}

impl OwnedSource {
    unsafe fn from_raw(source: &NdiSource) -> Option<Self> {
        if source.p_ndi_name.is_null() {
            return None;
        }

        let name = CStr::from_ptr(source.p_ndi_name)
            .to_string_lossy()
            .to_string();
        let url = if source.p_url_address.is_null() {
            None
        } else {
            Some(
                CStr::from_ptr(source.p_url_address)
                    .to_string_lossy()
                    .to_string(),
            )
        };

        Some(Self {
            name: CString::new(name).ok()?,
            url: url.and_then(|url| CString::new(url).ok()),
        })
    }

    fn name(&self) -> String {
        self.name.to_string_lossy().to_string()
    }

    fn as_ndi_source(&self) -> NdiSource {
        NdiSource {
            p_ndi_name: self.name.as_ptr(),
            p_url_address: self
                .url
                .as_ref()
                .map(|url| url.as_ptr())
                .unwrap_or(ptr::null()),
        }
    }
}

fn set_status(status: &Arc<Mutex<String>>, message: String) {
    if let Ok(mut status) = status.lock() {
        *status = message;
    }
}
