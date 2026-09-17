//! Remote UI host for snapshot streaming.

use core_types::ServiceId;
use ipc::ChannelId;
use ipc::{MessageEnvelope, MessagePayload, SchemaVersion};
use kernel_api::{KernelApi, KernelError};
use serde::{Deserialize, Serialize};
use services_gui_host::DesktopScene;
use services_workspace_manager::WorkspaceRenderSnapshot;
use std::io::Write;
use thiserror::Error;

const REMOTE_UI_ACTION: &str = "ui.snapshot";
const REMOTE_UI_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);
/// Graphical desktop scenes (GFX-041) travel as their own action so a viewer
/// that only understands text snapshots can ignore them.
pub const REMOTE_DESKTOP_ACTION: &str = "ui.desktop";
pub const REMOTE_DESKTOP_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// A graphical desktop frame: the scene data, not pixels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteDesktopFrame {
    pub revision: u64,
    pub timestamp_ns: u64,
    pub scene: DesktopScene,
}

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
        }
    }

    pub fn add_sink(&mut self, sink: Box<dyn SnapshotSink>) {
        self.sinks.push(sink);
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

        for sink in &mut self.sinks {
            sink.send(frame.clone())?;
        }

        Ok(frame)
    }

    /// Ship a graphical desktop scene to every sink. Shares the revision
    /// counter with text snapshots so a mixed stream stays totally ordered.
    pub fn push_desktop(
        &mut self,
        scene: DesktopScene,
        timestamp_ns: u64,
    ) -> Result<RemoteDesktopFrame, RemoteUiError> {
        self.revision += 1;
        let frame = RemoteDesktopFrame {
            revision: self.revision,
            timestamp_ns,
            scene,
        };
        for sink in &mut self.sinks {
            sink.send_desktop(frame.clone())?;
        }
        Ok(frame)
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
        assert_eq!(sink.desktop_frames[0].scene, scene);
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
        assert_eq!(decoded.scene, scene);
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
                assert_eq!(frame.scene, scene);
            }
            other => panic!("{other:?}"),
        }
        assert!(lines[1].starts_with(r#"{"kind":"desktop""#));
    }
}
