//! Remote UI host for snapshot streaming.

use core_types::ServiceId;
use ipc::ChannelId;
use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{KernelApi, KernelError};
use serde::{Deserialize, Serialize};
use services_gui_host::{DesktopScene, SceneEncoder, SceneUpdate};
use services_workspace_manager::WorkspaceRenderSnapshot;
use std::io::Write;
use thiserror::Error;

const REMOTE_UI_ACTION: &str = "ui.snapshot";
const REMOTE_UI_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);
/// Graphical desktop scenes (GFX-041) travel as their own action so a viewer
/// that only understands text snapshots can ignore them.
pub const REMOTE_DESKTOP_ACTION: &str = "ui.desktop";
pub const REMOTE_DESKTOP_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// A graphical desktop frame: a keyframe or delta (GFX-042), not pixels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteDesktopFrame {
    pub revision: u64,
    pub timestamp_ns: u64,
    pub update: SceneUpdate,
}

/// Default keyframe cadence for remote desktop streams.
pub const DEFAULT_KEYFRAME_INTERVAL: u32 = 60;

/// Snapshot frame streamed to remote UI clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSnapshotFrame {
    pub revision: u64,
    pub timestamp_ns: u64,
    pub snapshot: WorkspaceRenderSnapshot,
}

/// Errors for remote UI streaming.
#[derive(Debug, Error)]
pub enum RemoteUiError {
    #[error("Kernel error: {0}")]
    Kernel(String),

    #[error("Serialization error: {0}")]
    Encode(String),

    #[error("I/O error: {0}")]
    Io(String),
}

impl From<KernelError> for RemoteUiError {
    fn from(error: KernelError) -> Self {
        RemoteUiError::Kernel(error.to_string())
    }
}

/// Snapshot sink abstraction.
pub trait SnapshotSink {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError>;

    /// Receive a graphical desktop frame. Sinks that only carry text
    /// snapshots may keep the default, which drops the frame.
    fn send_desktop(&mut self, _frame: RemoteDesktopFrame) -> Result<(), RemoteUiError> {
        Ok(())
    }
}

/// Remote UI host that fans out snapshots to sinks.
pub struct RemoteUiHost {
    revision: u64,
    sinks: Vec<Box<dyn SnapshotSink>>,
    encoder: SceneEncoder,
}

impl Default for RemoteUiHost {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteUiHost {
    pub fn new() -> Self {
        Self {
            revision: 0,
            sinks: Vec::new(),
            encoder: SceneEncoder::new(DEFAULT_KEYFRAME_INTERVAL),
        }
    }

    /// How many viewers are still connected. A sink that fails a send is
    /// dropped, so this falls as viewers go away.
    pub fn sink_count(&self) -> usize {
        self.sinks.len()
    }

    pub fn add_sink(&mut self, sink: Box<dyn SnapshotSink>) {
        self.sinks.push(sink);
        // A viewer that joins mid-stream has no base scene, so the next
        // desktop frame has to be a keyframe -- which is what
        // `request_keyframe` is for, and its own doc comment says "(a viewer
        // joined)". Nothing called it on the one path where a viewer joins.
        // Until the cadence happened to come round, the new viewer was
        // decoding deltas against a scene it had never seen.
        self.request_keyframe();
    }

    pub fn push_snapshot(
        &mut self,
        snapshot: WorkspaceRenderSnapshot,
        timestamp_ns: u64,
    ) -> Result<RemoteSnapshotFrame, RemoteUiError> {
        self.revision += 1;
        let frame = RemoteSnapshotFrame {
            revision: self.revision,
            timestamp_ns,
            snapshot,
        };

        // A sink that fails is a viewer that has gone away. Propagating the
        // first error aborted the fan-out, so every sink later in the list
        // never saw that frame -- and the dead one was never removed, so
        // every later push failed at the same index and the whole remote UI
        // went dark for everyone because one viewer closed its window.
        self.sinks
            .retain_mut(|sink| sink.send(frame.clone()).is_ok());

        Ok(frame)
    }

    /// Ship a graphical desktop scene to every sink as a keyframe or delta.
    /// Shares the revision counter with text snapshots so a mixed stream
    /// stays totally ordered.
    pub fn push_desktop(
        &mut self,
        scene: DesktopScene,
        timestamp_ns: u64,
    ) -> Result<RemoteDesktopFrame, RemoteUiError> {
        self.revision += 1;
        let frame = RemoteDesktopFrame {
            revision: self.revision,
            timestamp_ns,
            update: self.encoder.encode(&scene),
        };
        // As above. This one also matters for correctness and not just
        // liveness: the encoder has already consumed the delta state, so a
        // sink skipped by an early return has a hole in its delta stream and
        // renders a corrupted desktop until the next keyframe.
        self.sinks
            .retain_mut(|sink| sink.send_desktop(frame.clone()).is_ok());
        Ok(frame)
    }

    /// Force the next desktop frame to be a keyframe (a viewer joined).
    pub fn request_keyframe(&mut self) {
        self.encoder.reset();
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// IPC sink for remote UI snapshots.
pub struct IpcSnapshotSink<K: KernelApi> {
    kernel: K,
    channel: ChannelId,
    destination: ServiceId,
}

impl<K: KernelApi> IpcSnapshotSink<K> {
    pub fn new(kernel: K, channel: ChannelId, destination: ServiceId) -> Self {
        Self {
            kernel,
            channel,
            destination,
        }
    }
}

impl<K: KernelApi> SnapshotSink for IpcSnapshotSink<K> {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
        let payload =
            MessagePayload::new(&frame).map_err(|err| RemoteUiError::Encode(err.to_string()))?;
        let message = MessageEnvelope::new(
            self.destination,
            REMOTE_UI_ACTION,
            REMOTE_UI_SCHEMA,
            payload,
        );
        self.kernel.send_message(self.channel, message)?;
        Ok(())
    }

    fn send_desktop(&mut self, frame: RemoteDesktopFrame) -> Result<(), RemoteUiError> {
        let payload =
            MessagePayload::new(&frame).map_err(|err| RemoteUiError::Encode(err.to_string()))?;
        let message = MessageEnvelope::new(
            self.destination,
            REMOTE_DESKTOP_ACTION,
            REMOTE_DESKTOP_SCHEMA,
            payload,
        );
        self.kernel.send_message(self.channel, message)?;
        Ok(())
    }
}

/// One JSON object per line; desktop frames are tagged so a reader can tell
/// the two frame kinds apart.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JsonLineRecord {
    Snapshot(RemoteSnapshotFrame),
    Desktop(RemoteDesktopFrame),
}

/// JSON-line sink for network transports.
pub struct JsonLineSink<W: Write> {
    writer: W,
}

impl<W: Write> JsonLineSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> JsonLineSink<W> {
    fn write_record(&mut self, record: &JsonLineRecord) -> Result<(), RemoteUiError> {
        serde_json::to_writer(&mut self.writer, record)
            .map_err(|err| RemoteUiError::Encode(err.to_string()))?;
        self.writer
            .write_all(b"\n")
            .map_err(|err| RemoteUiError::Io(err.to_string()))?;
        Ok(())
    }
}

impl<W: Write> SnapshotSink for JsonLineSink<W> {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
        self.write_record(&JsonLineRecord::Snapshot(frame))
    }

    fn send_desktop(&mut self, frame: RemoteDesktopFrame) -> Result<(), RemoteUiError> {
        self.write_record(&JsonLineRecord::Desktop(frame))
    }
}

/// In-memory sink for tests.
#[derive(Default)]
pub struct InMemorySink {
    pub frames: Vec<RemoteSnapshotFrame>,
    pub desktop_frames: Vec<RemoteDesktopFrame>,
}

impl SnapshotSink for InMemorySink {
    fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
        self.frames.push(frame);
        Ok(())
    }

    fn send_desktop(&mut self, frame: RemoteDesktopFrame) -> Result<(), RemoteUiError> {
        self.desktop_frames.push(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(debug_assertions)]
    use services_workspace_manager::DebugInfo;
    use std::sync::{Arc, Mutex};
    use view_types::{ViewContent, ViewFrame, ViewId, ViewKind};

    #[derive(Default)]
    struct MockKernel {
        sent: Arc<Mutex<Vec<(ChannelId, MessageEnvelope)>>>,
    }

    impl KernelApi for MockKernel {
        fn spawn_task(
            &mut self,
            _descriptor: kernel_api::TaskDescriptor,
        ) -> Result<kernel_api::TaskHandle, KernelError> {
            Err(KernelError::SpawnFailed("not supported".to_string()))
        }

        fn create_channel(&mut self) -> Result<ChannelId, KernelError> {
            Err(KernelError::ChannelError("not supported".to_string()))
        }

        fn send_message(
            &mut self,
            channel: ChannelId,
            message: MessageEnvelope,
        ) -> Result<(), KernelError> {
            let mut sent = self.sent.lock().expect("lock sent");
            sent.push((channel, message));
            Ok(())
        }

        fn receive_message(
            &mut self,
            _channel: ChannelId,
            _timeout: Option<kernel_api::Duration>,
        ) -> Result<MessageEnvelope, KernelError> {
            Err(KernelError::ReceiveFailed("not supported".to_string()))
        }

        fn now(&self) -> kernel_api::Instant {
            kernel_api::Instant::from_nanos(0)
        }

        fn sleep(&mut self, _duration: kernel_api::Duration) -> Result<(), KernelError> {
            Ok(())
        }

        fn grant_capability(
            &mut self,
            _task: core_types::TaskId,
            _capability: core_types::Cap<()>,
        ) -> Result<(), KernelError> {
            Ok(())
        }

        fn register_service(
            &mut self,
            _service_id: ServiceId,
            _channel: ChannelId,
        ) -> Result<(), KernelError> {
            Ok(())
        }

        fn lookup_service(&self, _service_id: ServiceId) -> Result<ChannelId, KernelError> {
            Err(KernelError::ServiceNotFound("not supported".to_string()))
        }
    }

    fn sample_view(kind: ViewKind, revision: u64, text: &str) -> ViewFrame {
        let content = match kind {
            ViewKind::TextBuffer => ViewContent::text_buffer(vec![text.to_string()]),
            ViewKind::StatusLine => ViewContent::status_line(text),
            ViewKind::Panel => ViewContent::panel(text),
        };

        ViewFrame::new(ViewId::new(), kind, revision, content, 1000 + revision)
    }

    fn sample_snapshot() -> WorkspaceRenderSnapshot {
        let main_view = sample_view(ViewKind::TextBuffer, 1, "tile 0");
        let status_view = sample_view(ViewKind::StatusLine, 2, "focused tile");
        let composed_main_view = sample_view(ViewKind::TextBuffer, 3, "composed tiles");
        let composed_status_view = sample_view(ViewKind::StatusLine, 4, "layout: split");

        WorkspaceRenderSnapshot {
            focused_component: None,
            main_view: Some(main_view.clone()),
            status_view: Some(status_view.clone()),
            composed_main_view: Some(composed_main_view.clone()),
            composed_status_view: Some(composed_status_view.clone()),
            layout: services_workspace_manager::WorkspaceLayoutSnapshot::default(),
            tiles: vec![services_workspace_manager::WorkspaceTileRenderSnapshot {
                tile_index: 0,
                is_focused: true,
                active_component: None,
                tabs: Vec::new(),
                main_view: Some(main_view),
                status_view: Some(status_view),
            }],
            component_count: 1,
            running_count: 1,
            status_strip: "Workspace - 1 tile - Idle".to_string(),
            breadcrumbs: "PANDA > ROOT".to_string(),
            #[cfg(debug_assertions)]
            debug_info: Some(DebugInfo {
                focused_component_name: Some("editor".to_string()),
                focused_component_type: None,
                last_key_event: Some("ctrl+w".to_string()),
                last_routed_to: None,
                consumed_by_global: false,
            }),
        }
    }

    /// A viewer that has closed its window.
    struct DeadSink;
    impl SnapshotSink for DeadSink {
        fn send(&mut self, _frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
            Err(RemoteUiError::Io("broken pipe".to_string()))
        }
        fn send_desktop(&mut self, _frame: RemoteDesktopFrame) -> Result<(), RemoteUiError> {
            Err(RemoteUiError::Io("broken pipe".to_string()))
        }
    }

    #[test]
    fn one_dead_viewer_does_not_take_the_others_with_it() {
        // The fan-out propagated the first sink's error, so every sink after
        // it never saw the frame -- and the dead one was never removed, so
        // every later push failed at the same index. One viewer closing its
        // window took the whole remote UI down for everybody, permanently.
        let mut host = RemoteUiHost::new();
        host.add_sink(Box::new(DeadSink));
        host.add_sink(Box::new(InMemorySink::default()));

        host.push_snapshot(sample_snapshot(), 10)
            .expect("a dead viewer must not fail the push");
        host.push_snapshot(sample_snapshot(), 11)
            .expect("and must not keep failing it");
        assert_eq!(
            host.sink_count(),
            1,
            "the viewer that went away should have been dropped"
        );
    }

    #[test]
    fn test_remote_ui_host_revision_increments() {
        let mut host = RemoteUiHost::new();
        let sink = InMemorySink::default();
        host.add_sink(Box::new(sink));

        let snapshot = sample_snapshot();

        let frame1 = host.push_snapshot(snapshot.clone(), 10).unwrap();
        let frame2 = host.push_snapshot(snapshot, 11).unwrap();

        assert_eq!(frame1.revision, 1);
        assert_eq!(frame2.revision, 2);
        assert!(frame2.snapshot.composed_main_view.is_some());
        assert_eq!(frame2.snapshot.tiles.len(), 1);
    }

    #[test]
    fn test_ipc_snapshot_sink_sends_message() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let kernel = MockKernel { sent: sent.clone() };
        let channel = ChannelId::new();
        let destination = ServiceId::new();

        let mut sink = IpcSnapshotSink::new(kernel, channel, destination);

        let frame = RemoteSnapshotFrame {
            revision: 1,
            timestamp_ns: 5,
            snapshot: sample_snapshot(),
        };

        sink.send(frame).unwrap();

        let sent = sent.lock().expect("lock sent");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, channel);
        assert_eq!(sent[0].1.action, REMOTE_UI_ACTION.to_string());
    }

    #[test]
    fn test_json_line_sink_round_trips_layout_snapshot_fields() {
        let mut output = Vec::new();
        let mut sink = JsonLineSink::new(&mut output);
        let frame = RemoteSnapshotFrame {
            revision: 7,
            timestamp_ns: 99,
            snapshot: sample_snapshot(),
        };

        sink.send(frame.clone()).unwrap();

        let encoded = String::from_utf8(output).expect("utf8 json line");
        let decoded: RemoteSnapshotFrame =
            serde_json::from_str(encoded.trim_end()).expect("decode frame");

        assert_eq!(decoded.revision, frame.revision);
        assert!(decoded.snapshot.composed_status_view.is_some());
        assert_eq!(decoded.snapshot.layout.focused_tile, 0);
        assert_eq!(decoded.snapshot.tiles.len(), 1);
        assert_eq!(decoded.snapshot.status_strip, frame.snapshot.status_strip);
    }

    fn sample_scene() -> DesktopScene {
        use services_gui_host::{DesktopCursor, DesktopWindow, SurfaceRect, SurfaceSize};
        let frame = sample_view(ViewKind::TextBuffer, 9, "remote");
        DesktopScene::new(
            SurfaceSize::new(20, 8),
            vec![DesktopWindow::new(frame, SurfaceRect::new(1, 1, 10, 5)).focused()],
        )
        .with_cursor(Some(DesktopCursor::new(3, 4)))
    }

    #[test]
    fn test_push_desktop_shares_revision_and_reaches_every_sink() {
        let mut host = RemoteUiHost::new();
        let sink = Arc::new(Mutex::new(InMemorySink::default()));
        struct Shared(Arc<Mutex<InMemorySink>>);
        impl SnapshotSink for Shared {
            fn send(&mut self, frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
                self.0.lock().unwrap().send(frame)
            }
            fn send_desktop(&mut self, frame: RemoteDesktopFrame) -> Result<(), RemoteUiError> {
                self.0.lock().unwrap().send_desktop(frame)
            }
        }
        host.add_sink(Box::new(Shared(sink.clone())));

        host.push_snapshot(sample_snapshot(), 10).unwrap();
        let scene = sample_scene();
        let frame = host.push_desktop(scene.clone(), 20).unwrap();
        assert_eq!(
            frame.revision, 2,
            "desktop frames continue the same revision stream"
        );
        assert_eq!(host.revision(), 2);
        let sink = sink.lock().unwrap();
        assert_eq!(sink.frames.len(), 1);
        assert_eq!(sink.desktop_frames.len(), 1);
        assert_eq!(sink.desktop_frames[0], frame);
        assert_eq!(sink.desktop_frames[0].update, as_keyframe(&scene));
    }

    /// `scene` as a keyframe carries it: a keyframe replaces the viewer's
    /// whole surface, so its damage is the whole surface rather than
    /// whatever the producer thought had changed.
    fn as_keyframe(scene: &DesktopScene) -> SceneUpdate {
        let (w, h) = scene.pixel_size();
        let mut scene = scene.clone();
        scene.damage = Some(graphics_rasterizer::RasterRect::new(0, 0, w, h));
        SceneUpdate::Keyframe(scene)
    }

    #[test]
    fn test_ipc_sink_sends_desktop_frames_with_their_own_action() {
        let kernel = MockKernel::default();
        let sent = kernel.sent.clone();
        let mut host = RemoteUiHost::new();
        host.add_sink(Box::new(IpcSnapshotSink::new(
            kernel,
            ChannelId::new(),
            ServiceId::new(),
        )));
        let scene = sample_scene();
        host.push_desktop(scene.clone(), 5).unwrap();
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1.action, REMOTE_DESKTOP_ACTION);
        let decoded: RemoteDesktopFrame = sent[0].1.payload.deserialize().unwrap();
        assert_eq!(decoded.update, as_keyframe(&scene));
    }

    #[test]
    fn test_json_line_sink_tags_frame_kinds_and_round_trips() {
        let scene = sample_scene();
        #[derive(Clone, Default)]
        struct SharedBuffer(Arc<Mutex<Vec<u8>>>);
        impl Write for SharedBuffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buffer = SharedBuffer::default();
        let mut host = RemoteUiHost::new();
        host.add_sink(Box::new(JsonLineSink::new(buffer.clone())));
        host.push_snapshot(sample_snapshot(), 1).unwrap();
        host.push_desktop(scene.clone(), 2).unwrap();
        let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: JsonLineRecord = serde_json::from_str(lines[0]).unwrap();
        let second: JsonLineRecord = serde_json::from_str(lines[1]).unwrap();
        assert!(matches!(first, JsonLineRecord::Snapshot(_)));
        match second {
            JsonLineRecord::Desktop(frame) => {
                assert_eq!(frame.revision, 2);
                assert_eq!(frame.update, as_keyframe(&scene));
            }
            other => panic!("{other:?}"),
        }
        assert!(lines[1].starts_with(r#"{"kind":"desktop""#));
    }

    #[test]
    fn test_second_desktop_frame_is_a_delta_until_a_keyframe_is_requested() {
        use services_gui_host::{DesktopCursor, SceneDecoder};
        let mut host = RemoteUiHost::new();
        let first = host.push_desktop(sample_scene(), 1).unwrap();
        let mut moved = sample_scene();
        moved.cursor = Some(DesktopCursor::new(7, 7));
        let second = host.push_desktop(moved.clone(), 2).unwrap();
        assert!(matches!(first.update, SceneUpdate::Keyframe(_)));
        assert!(matches!(second.update, SceneUpdate::Delta(_)));

        let mut decoder = SceneDecoder::new();
        decoder.apply(&first.update).unwrap();
        let mut got = decoder.apply(&second.update).unwrap().clone();
        got.damage = None;
        assert_eq!(got, moved);

        host.request_keyframe();
        let third = host.push_desktop(moved, 3).unwrap();
        assert!(matches!(third.update, SceneUpdate::Keyframe(_)));
        assert_eq!(third.revision, 3);
    }

    #[test]
    fn test_json_line_session_replays_to_identical_scenes() {
        use services_gui_host::{DesktopCursor, SceneReplay};
        #[derive(Clone, Default)]
        struct SharedBuffer(Arc<Mutex<Vec<u8>>>);
        impl Write for SharedBuffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buffer = SharedBuffer::default();
        let mut host = RemoteUiHost::new();
        host.add_sink(Box::new(JsonLineSink::new(buffer.clone())));

        let mut live = Vec::new();
        for i in 0..5u64 {
            let mut scene = sample_scene();
            scene.cursor = Some(DesktopCursor::new(3 + i as usize, 4));
            host.push_desktop(scene.clone(), i).unwrap();
            live.push(scene);
        }
        // Interleaved text snapshots must not disturb the desktop stream.
        host.push_snapshot(sample_snapshot(), 99).unwrap();

        let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        let mut updates = Vec::new();
        let mut revisions = Vec::new();
        for line in text.lines() {
            if let JsonLineRecord::Desktop(frame) = serde_json::from_str(line).unwrap() {
                revisions.push(frame.revision);
                updates.push(frame.update);
            }
        }
        assert_eq!(revisions, vec![1, 2, 3, 4, 5]);
        let scenes = SceneReplay::new(updates).replay_all().unwrap();
        assert_eq!(scenes.len(), live.len());
        for (got, expected) in scenes.iter().zip(&live) {
            assert_eq!(got.cursor, expected.cursor);
            assert_eq!(got.windows, expected.windows);
        }
    }
}

#[cfg(test)]
mod joining_viewer_tests {
    use super::*;
    use services_gui_host::transport::{SceneDecoder, SceneUpdate};
    use services_gui_host::{DesktopScene, DesktopWindow, SurfaceSize};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Recorder(Arc<Mutex<Vec<RemoteDesktopFrame>>>);

    impl SnapshotSink for Recorder {
        fn send(&mut self, _frame: RemoteSnapshotFrame) -> Result<(), RemoteUiError> {
            Ok(())
        }
        fn send_desktop(&mut self, frame: RemoteDesktopFrame) -> Result<(), RemoteUiError> {
            self.0.lock().unwrap().push(frame);
            Ok(())
        }
    }

    fn scene(width: usize, windows: Vec<DesktopWindow>) -> DesktopScene {
        DesktopScene {
            size: SurfaceSize::new(width, 24),
            windows,
            cursor: None,
            theme: None,
            damage: None,
            wallpaper: None,
        }
    }

    /// The finding: a viewer that joins mid-stream was sent the next frame as
    /// a *delta*, against a scene it had never received. `request_keyframe`
    /// existed for exactly this and nothing called it when a viewer joined,
    /// so the new viewer decoded garbage until the keyframe cadence came
    /// round -- up to `DEFAULT_KEYFRAME_INTERVAL` frames later.
    #[test]
    fn a_viewer_that_joins_mid_stream_gets_a_keyframe_first() {
        let mut host = RemoteUiHost::new();
        // Stream a few frames to an existing viewer, so the encoder has a
        // base and would otherwise send deltas.
        let first = Arc::new(Mutex::new(Vec::new()));
        host.add_sink(Box::new(Recorder(first)));
        host.push_desktop(scene(80, Vec::new()), 1).unwrap();
        host.push_desktop(scene(80, Vec::new()), 2).unwrap();

        let joined = Arc::new(Mutex::new(Vec::new()));
        host.add_sink(Box::new(Recorder(joined.clone())));
        host.push_desktop(scene(80, Vec::new()), 3).unwrap();

        let frames = joined.lock().unwrap();
        let first_seen = frames.first().expect("the new viewer received nothing");
        assert!(
            matches!(first_seen.update, SceneUpdate::Keyframe(_)),
            "a viewer that just joined was sent a delta against a scene it \
             has never seen"
        );

        // And it decodes, which is the consequence that matters.
        let mut decoder = SceneDecoder::default();
        decoder
            .apply(&first_seen.update)
            .expect("the first frame a new viewer sees must decode on its own");
    }

    /// A keyframe replaces the whole scene, so its damage must say so. It
    /// used to carry the producer's "what changed since last frame", which a
    /// viewer honouring damage would use to repaint one rectangle of an
    /// otherwise blank surface.
    #[test]
    fn a_keyframes_damage_covers_the_whole_surface() {
        let mut host = RemoteUiHost::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        host.add_sink(Box::new(Recorder(seen.clone())));

        let mut narrow = scene(80, Vec::new());
        // A producer that thinks only a small corner changed.
        narrow.damage = Some(graphics_rasterizer::RasterRect::new(3, 4, 5, 6));
        host.push_desktop(narrow.clone(), 1).unwrap();

        let frames = seen.lock().unwrap();
        let SceneUpdate::Keyframe(sent) = &frames[0].update else {
            panic!("the first frame to a new viewer must be a keyframe");
        };
        let (w, h) = narrow.pixel_size();
        assert_eq!(
            sent.damage,
            Some(graphics_rasterizer::RasterRect::new(0, 0, w, h)),
            "a keyframe carried a partial damage rectangle"
        );
    }
}
