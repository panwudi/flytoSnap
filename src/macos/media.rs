use block2::RcBlock;
use dispatch2::DispatchQueue;
use image::{
    Delay, Frame, RgbaImage,
    codecs::gif::{GifEncoder, Repeat},
};
use objc2::{
    AnyThread, DefinedClass, define_class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyObject, ProtocolObject},
};
use objc2_av_foundation::*;
use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::*;
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSURL,
};
use objc2_screen_capture_kit::*;
use std::{
    fs::File,
    path::{Path, PathBuf},
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Sender},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mp4,
    Gif,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Gif => "gif",
        }
    }
    pub fn fps(self) -> i32 {
        if self == Self::Gif { 10 } else { 30 }
    }
}
pub enum Event {
    Started(u64),
    Finished {
        job: u64,
        path: PathBuf,
        format: Format,
        cancelled: bool,
    },
    Failed(u64, String),
}

pub struct Handle {
    inner: Arc<Mutex<Inner>>,
    pub frames: Arc<AtomicU64>,
}
struct Inner {
    stream: Option<Retained<SCStream>>,
    output: Option<Retained<Output>>,
    encoder: Option<Encoder>,
    stopping: bool,
    path: PathBuf,
    format: Format,
    job: u64,
    tx: Sender<Event>,
}
// All accesses to the native stream and writer are serialized by this mutex.
// ScreenCaptureKit control methods are asynchronous; AVAssetWriter is used on one serialized path.
unsafe impl Send for Inner {}
impl Clone for Handle {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            frames: self.frames.clone(),
        }
    }
}

enum Encoder {
    Mp4 {
        writer: Retained<AVAssetWriter>,
        input: Retained<AVAssetWriterInput>,
        started: bool,
        count: u64,
    },
    Gif {
        writer: GifEncoder<File>,
        pending: Option<(RgbaImage, f64, Instant)>,
        count: u64,
    },
}
impl Encoder {
    fn new(path: &Path, format: Format, width: usize, height: usize) -> Result<Self, String> {
        unsafe {
            if format == Format::Gif {
                let mut writer = GifEncoder::new(File::create(path).map_err(|e| e.to_string())?);
                writer
                    .set_repeat(Repeat::Infinite)
                    .map_err(|e| e.to_string())?;
                return Ok(Self::Gif {
                    writer,
                    pending: None,
                    count: 0,
                });
            }
            let url = NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(
                &path.to_string_lossy(),
            ));
            let writer = AVAssetWriter::initWithURL_fileType_error(
                AVAssetWriter::alloc(),
                &url,
                AVFileTypeMPEG4.ok_or("Required system video API unavailable")?,
            )
            .map_err(|e| e.localizedDescription().to_string())?;
            let w = NSNumber::new_usize(width);
            let h = NSNumber::new_usize(height);
            let settings: Retained<NSDictionary<objc2_foundation::NSString, AnyObject>> =
                NSDictionary::from_slices(
                    &[
                        AVVideoCodecKey.ok_or("Required system video API unavailable")?,
                        AVVideoWidthKey.ok_or("Required system video API unavailable")?,
                        AVVideoHeightKey.ok_or("Required system video API unavailable")?,
                    ],
                    &[
                        AVVideoCodecTypeH264.ok_or("Required system video API unavailable")?
                            as &AnyObject,
                        &*w as &AnyObject,
                        &*h as &AnyObject,
                    ],
                );
            let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                AVMediaTypeVideo.ok_or("Required system video API unavailable")?,
                Some(&settings),
            );
            input.setExpectsMediaDataInRealTime(true);
            if !writer.canAddInput(&input) {
                return Err("The system video encoder rejected this region".into());
            }
            writer.addInput(&input);
            if !writer.startWriting() {
                return Err(writer
                    .error()
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_else(|| "Could not start the system video encoder".into()));
            }
            Ok(Self::Mp4 {
                writer,
                input,
                started: false,
                count: 0,
            })
        }
    }
    fn append(&mut self, sample: &CMSampleBuffer) -> Result<bool, String> {
        unsafe {
            let Some(image) = sample.image_buffer() else {
                return Ok(false);
            };
            match self {
                Self::Mp4 {
                    writer,
                    input,
                    started,
                    count,
                } => {
                    if !input.isReadyForMoreMediaData() {
                        return Ok(false);
                    }
                    if !*started {
                        writer.startSessionAtSourceTime(sample.presentation_time_stamp());
                        *started = true;
                    }
                    if !input.appendSampleBuffer(sample) {
                        return Err(writer
                            .error()
                            .map(|e| e.localizedDescription().to_string())
                            .unwrap_or_else(|| "Could not encode the video frame".into()));
                    }
                    *count += 1;
                    Ok(true)
                }
                Self::Gif {
                    writer,
                    pending,
                    count,
                } => {
                    let time = sample.presentation_time_stamp().seconds();
                    if pending
                        .as_ref()
                        .is_some_and(|(_, last, _)| time - *last < 0.075)
                    {
                        return Ok(false);
                    }
                    let pixel = &*CFRetained::as_ptr(&image).as_ptr().cast::<CVPixelBuffer>();
                    let rgba = rgba_from_pixel(pixel)?;
                    let scale = (1280.0 / rgba.width() as f64).min(1.0);
                    let rgba = if scale < 1.0 {
                        image::imageops::resize(
                            &rgba,
                            (rgba.width() as f64 * scale) as u32,
                            (rgba.height() as f64 * scale) as u32,
                            image::imageops::FilterType::Triangle,
                        )
                    } else {
                        rgba
                    };
                    if let Some((previous, last, _)) = pending.take() {
                        let delay = ((time - last) * 1000.0).round().clamp(10.0, 655_000.0) as u32;
                        writer
                            .encode_frame(Frame::from_parts(
                                previous,
                                0,
                                0,
                                Delay::from_numer_denom_ms(delay, 1),
                            ))
                            .map_err(|e| e.to_string())?;
                    }
                    *pending = Some((rgba, time, Instant::now()));
                    *count += 1;
                    Ok(true)
                }
            }
        }
    }
    fn finish(mut self) -> Result<(), String> {
        unsafe {
            match &mut self {
                Self::Gif {
                    writer,
                    pending,
                    count,
                } => {
                    if *count == 0 {
                        return Err("No screen frames were captured".into());
                    }
                    if let Some((frame, _, when)) = pending.take() {
                        let delay = (when.elapsed().as_millis() as u32).clamp(100, 655_000);
                        writer
                            .encode_frame(Frame::from_parts(
                                frame,
                                0,
                                0,
                                Delay::from_numer_denom_ms(delay, 1),
                            ))
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(())
                }
                Self::Mp4 {
                    writer,
                    input,
                    started,
                    count,
                } => {
                    if !*started || *count == 0 {
                        writer.cancelWriting();
                        return Err("No screen frames were captured".into());
                    }
                    input.markAsFinished();
                    let (tx, rx) = mpsc::channel();
                    let done = RcBlock::new(move || {
                        let _ = tx.send(());
                    });
                    writer.finishWritingWithCompletionHandler(&done);
                    if rx.recv_timeout(Duration::from_secs(45)).is_err() {
                        writer.cancelWriting();
                        return Err("Video finalization timed out".into());
                    }
                    if writer.status() != AVAssetWriterStatus::Completed {
                        return Err(writer
                            .error()
                            .map(|e| e.localizedDescription().to_string())
                            .unwrap_or_else(|| "Could not finalize the MP4 file".into()));
                    }
                    Ok(())
                }
            }
        }
    }
}
fn rgba_from_pixel(pixel: &CVPixelBuffer) -> Result<RgbaImage, String> {
    unsafe {
        if CVPixelBufferGetPixelFormatType(pixel) != kCVPixelFormatType_32BGRA {
            return Err("Unexpected screen pixel format".into());
        }
        if CVPixelBufferLockBaseAddress(pixel, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return Err("Could not access the screen frame".into());
        }
        let w = CVPixelBufferGetWidth(pixel);
        let h = CVPixelBufferGetHeight(pixel);
        let stride = CVPixelBufferGetBytesPerRow(pixel);
        let base = CVPixelBufferGetBaseAddress(pixel).cast::<u8>();
        if base.is_null() || stride < w * 4 {
            CVPixelBufferUnlockBaseAddress(pixel, CVPixelBufferLockFlags::ReadOnly);
            return Err("Invalid pixel buffer".into());
        }
        let mut result = RgbaImage::new(w as u32, h as u32);
        for y in 0..h {
            let row = std::slice::from_raw_parts(base.add(y * stride), w * 4);
            for x in 0..w {
                let i = x * 4;
                result.put_pixel(
                    x as u32,
                    y as u32,
                    image::Rgba([row[i + 2], row[i + 1], row[i], 255]),
                );
            }
        }
        CVPixelBufferUnlockBaseAddress(pixel, CVPixelBufferLockFlags::ReadOnly);
        Ok(result)
    }
}

struct OutputIvars {
    handle: Handle,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[ivars=OutputIvars]
    struct Output;
    unsafe impl NSObjectProtocol for Output {}
    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn frame(&self, _stream: &SCStream, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
            if kind != SCStreamOutputType::Screen {
                return;
            }
            autoreleasepool(|_| {
                let handle = &self.ivars().handle;
                let mut inner = handle.inner.lock().unwrap();
                if inner.stopping {
                    return;
                }
                if let Some(encoder) = inner.encoder.as_mut() {
                    match encoder.append(sample) {
                        Ok(true) => {
                            handle.frames.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            inner.stopping = true;
                            let _ = inner.tx.send(Event::Failed(inner.job, error));
                        }
                    }
                }
            });
        }
    }
    unsafe impl SCStreamDelegate for Output {
        #[unsafe(method(stream:didStopWithError:))]
        fn stopped(&self, _stream: &SCStream, error: &NSError) {
            let inner = self.ivars().handle.inner.lock().unwrap();
            if !inner.stopping {
                let _ = inner.tx.send(Event::Failed(
                    inner.job,
                    error.localizedDescription().to_string(),
                ));
            }
        }
    }
);

pub struct Options {
    pub display_id: u32,
    pub region: CGRect,
    pub width: usize,
    pub height: usize,
    pub format: Format,
    pub shows_cursor: bool,
}
pub fn start(options: Options, job: u64, tx: Sender<Event>) -> Result<Handle, String> {
    let Options {
        display_id,
        region,
        width,
        height,
        format,
        shows_cursor,
    } = options;
    let directory = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
        .join("Library/Caches/flytoSnap/Recordings");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = directory.join(format!("flytoSnap-{stamp}.{}", format.extension()));
    let encoder = Encoder::new(&path, format, width, height)?;
    let handle = Handle {
        inner: Arc::new(Mutex::new(Inner {
            stream: None,
            output: None,
            encoder: Some(encoder),
            stopping: false,
            path,
            format,
            job,
            tx: tx.clone(),
        })),
        frames: Arc::new(AtomicU64::new(0)),
    };
    let returned = handle.clone();
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            autoreleasepool(|_| unsafe {
                let Some(content) = content.as_ref() else {
                    let _ = tx.send(Event::Failed(
                        job,
                        error
                            .as_ref()
                            .map(|e| e.localizedDescription().to_string())
                            .unwrap_or_else(|| "No shareable screen content".into()),
                    ));
                    return;
                };
                let Some(display) = content
                    .displays()
                    .iter()
                    .find(|d| d.displayID() == display_id)
                else {
                    let _ = tx.send(Event::Failed(
                        job,
                        "The selected display was disconnected".into(),
                    ));
                    return;
                };
                let excluded = NSArray::from_retained_slice(
                    &content
                        .applications()
                        .iter()
                        .filter(|a| a.processID() == std::process::id() as i32)
                        .collect::<Vec<_>>(),
                );
                let filter =
                    SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
                        SCContentFilter::alloc(),
                        &display,
                        &excluded,
                        &NSArray::new(),
                    );
                let config = SCStreamConfiguration::new();
                config.setSourceRect(region);
                config.setWidth(width);
                config.setHeight(height);
                config.setShowsCursor(shows_cursor);
                config.setPixelFormat(kCVPixelFormatType_32BGRA);
                config.setQueueDepth(4);
                config.setMinimumFrameInterval(CMTime::new(1, format.fps()));
                config.setColorSpaceName(objc2_core_graphics::kCGColorSpaceSRGB);
                let this = Output::alloc().set_ivars(OutputIvars {
                    handle: handle.clone(),
                });
                let output: Retained<Output> = msg_send![super(this), init];
                let stream = SCStream::initWithFilter_configuration_delegate(
                    SCStream::alloc(),
                    &filter,
                    &config,
                    Some(ProtocolObject::from_ref(&*output)),
                );
                let queue = DispatchQueue::new("com.flyto.snap.recording", None);
                if let Err(error) = stream.addStreamOutput_type_sampleHandlerQueue_error(
                    ProtocolObject::from_ref(&*output),
                    SCStreamOutputType::Screen,
                    Some(&queue),
                ) {
                    let _ = tx.send(Event::Failed(job, error.localizedDescription().to_string()));
                    return;
                }
                {
                    let mut inner = handle.inner.lock().unwrap();
                    if inner.stopping {
                        return;
                    }
                    inner.stream = Some(stream.clone());
                    inner.output = Some(output);
                }
                let tx = tx.clone();
                let complete = RcBlock::new(move |error: *mut NSError| {
                    if let Some(error) = error.as_ref() {
                        let _ =
                            tx.send(Event::Failed(job, error.localizedDescription().to_string()));
                    } else {
                        let _ = tx.send(Event::Started(job));
                    }
                });
                stream.startCaptureWithCompletionHandler(Some(&complete));
            });
        },
    );
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false,true,&handler);
    }
    Ok(returned)
}
impl Handle {
    pub fn stop(&self, cancelled: bool) {
        let stream = {
            let mut inner = self.inner.lock().unwrap();
            inner.stopping = true;
            inner.stream.clone()
        };
        let handle = self.clone();
        if let Some(stream) = stream {
            let stopped = RcBlock::new(move |_error: *mut NSError| {
                let handle = handle.clone();
                std::thread::spawn(move || handle.finalize(cancelled));
            });
            unsafe {
                stream.stopCaptureWithCompletionHandler(Some(&stopped));
            }
        } else {
            std::thread::spawn(move || handle.finalize(cancelled));
        }
    }
    fn finalize(&self, cancelled: bool) {
        let (encoder, path, format, job, tx) = {
            let mut inner = self.inner.lock().unwrap();
            inner.stream = None;
            inner.output = None;
            (
                inner.encoder.take(),
                inner.path.clone(),
                inner.format,
                inner.job,
                inner.tx.clone(),
            )
        };
        let result = encoder
            .map(|e| autoreleasepool(|_| e.finish()))
            .unwrap_or(Ok(()));
        if cancelled {
            let _ = std::fs::remove_file(&path);
            let _ = tx.send(Event::Finished {
                job,
                path,
                format,
                cancelled: true,
            });
        } else {
            match result {
                Ok(()) => {
                    let _ = tx.send(Event::Finished {
                        job,
                        path,
                        format,
                        cancelled: false,
                    });
                }
                Err(error) => {
                    let _ = tx.send(Event::Failed(job, error));
                }
            }
        }
    }
}

fn sample_from_rgba(
    image: &RgbaImage,
    index: i64,
    fps: i32,
) -> Result<CFRetained<CMSampleBuffer>, String> {
    unsafe {
        let mut raw = std::ptr::null_mut();
        let result = CVPixelBufferCreate(
            None,
            image.width() as usize,
            image.height() as usize,
            kCVPixelFormatType_32BGRA,
            None,
            NonNull::from(&mut raw),
        );
        if result != 0 {
            return Err(format!("Could not create test pixel buffer: {result}"));
        }
        let pixel = CFRetained::from_raw(NonNull::new(raw).ok_or("Missing pixel buffer")?);
        CVPixelBufferLockBaseAddress(&pixel, CVPixelBufferLockFlags::empty());
        let base = CVPixelBufferGetBaseAddress(&pixel).cast::<u8>();
        let stride = CVPixelBufferGetBytesPerRow(&pixel);
        for y in 0..image.height() as usize {
            for x in 0..image.width() as usize {
                let p = image.get_pixel(x as u32, y as u32);
                let dest = base.add(y * stride + x * 4);
                *dest = p[2];
                *dest.add(1) = p[1];
                *dest.add(2) = p[0];
                *dest.add(3) = 255;
            }
        }
        CVPixelBufferUnlockBaseAddress(&pixel, CVPixelBufferLockFlags::empty());
        let mut description = std::ptr::null();
        let result = objc2_core_media::CMVideoFormatDescriptionCreateForImageBuffer(
            None,
            &pixel,
            NonNull::from(&mut description),
        );
        if result != 0 {
            return Err(format!("Could not describe test pixels: {result}"));
        }
        let description = CFRetained::from_raw(NonNull::new(description.cast_mut()).unwrap());
        let mut timing = objc2_core_media::CMSampleTimingInfo {
            duration: CMTime::new(1, fps),
            presentationTimeStamp: CMTime::new(index, fps),
            decodeTimeStamp: objc2_core_media::kCMTimeInvalid,
        };
        let mut sample = std::ptr::null_mut();
        let result = CMSampleBuffer::create_ready_with_image_buffer(
            None,
            &pixel,
            &description,
            NonNull::from(&mut timing),
            NonNull::from(&mut sample),
        );
        if result != 0 {
            return Err(format!("Could not create test sample: {result}"));
        }
        Ok(CFRetained::from_raw(NonNull::new(sample).unwrap()))
    }
}
pub fn verify(output: &Path) -> Result<(), String> {
    std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
    for format in [Format::Gif, Format::Mp4] {
        let path = output.join(format!("recording-test.{}", format.extension()));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        let mut encoder = Encoder::new(&path, format, 320, 180)?;
        for i in 0..12 {
            let image = RgbaImage::from_fn(320, 180, |x, y| {
                image::Rgba([
                    ((x + i * 13) % 255) as u8,
                    ((y + i * 19) % 255) as u8,
                    80,
                    255,
                ])
            });
            let sample = sample_from_rgba(&image, i as i64, 10)?;
            let until = Instant::now() + Duration::from_secs(3);
            loop {
                if encoder.append(&sample)? {
                    break;
                }
                if Instant::now() > until {
                    return Err("Test encoder never became ready".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        encoder.finish()?;
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if format == Format::Mp4 {
            if !bytes.windows(4).any(|w| w == b"moov") || !bytes.windows(4).any(|w| w == b"mdat") {
                return Err("MP4 is missing finalized media atoms".into());
            }
        } else {
            use image::AnimationDecoder;
            let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
                File::open(&path).map_err(|e| e.to_string())?,
            ))
            .map_err(|e| e.to_string())?;
            let frames = decoder
                .into_frames()
                .collect_frames()
                .map_err(|e| e.to_string())?;
            if frames.len() < 10 {
                return Err("GIF animation contains too few frames".into());
            }
        }
    }
    println!("Media encoding check passed: animated GIF and finalized H.264 MP4.");
    Ok(())
}
