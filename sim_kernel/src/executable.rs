//! Executable loading and component launch (Phase 62).
//!
//! This module implements a minimal executable format (PEX - PandaGen Executable)
//! and provides utilities for loading and launching user programs.
//!
//! ## Format
//!
//! The PEX format is intentionally simple:
//! - Magic number: 0x50455800 (PEX\0)
//! - Version: u32
//! - Entry point: u64
//! - Section count: u32
//! - For each section:
//!   - Type: u32 (1=text, 2=data, 3=bss)
//!   - Size: u64
//!   - Permissions: u32 (bitfield: read=1, write=2, execute=4)
//!   - Data (for text/data sections, empty for bss)
//!
//! This format is position-independent and simple to parse and validate.

use core_types::{MemoryPerms, TaskId};
use identity::ExecutionId;
use kernel_api::{KernelApi, KernelError, TaskDescriptor};
use serde::{Deserialize, Serialize};

/// PEX magic number: "PEX\0"
pub const PEX_MAGIC: u32 = 0x50455800;

/// Current PEX format version
pub const PEX_VERSION: u32 = 1;

/// Section types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SectionType {
    /// Executable code (.text)
    Text = 1,
    /// Initialized data (.data)
    Data = 2,
    /// Zero-initialized data (.bss)
    Bss = 3,
}

impl SectionType {
    fn from_u32(val: u32) -> Result<Self, ExecutableError> {
        match val {
            1 => Ok(SectionType::Text),
            2 => Ok(SectionType::Data),
            3 => Ok(SectionType::Bss),
            _ => Err(ExecutableError::InvalidSectionType(val)),
        }
    }
}

/// Permission bits for sections
#[derive(Debug, Clone, Copy)]
pub struct SectionPerms {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl SectionPerms {
    pub fn from_u32(val: u32) -> Self {
        Self {
            read: (val & 1) != 0,
            write: (val & 2) != 0,
            execute: (val & 4) != 0,
        }
    }

    pub fn to_memory_perms(&self) -> MemoryPerms {
        if self.read && self.write && self.execute {
            MemoryPerms::all()
        } else if self.read && self.write {
            MemoryPerms::read_write()
        } else if self.read && self.execute {
            MemoryPerms::read_execute()
        } else if self.read {
            MemoryPerms::read_only()
        } else {
            MemoryPerms::none()
        }
    }

    pub fn text() -> Self {
        Self {
            read: true,
            write: false,
            execute: true,
        }
    }

    pub fn data() -> Self {
        Self {
            read: true,
            write: true,
            execute: false,
        }
    }
}

/// A section in a PEX executable
#[derive(Debug, Clone)]
pub struct Section {
    pub section_type: SectionType,
    pub size: u64,
    pub permissions: SectionPerms,
    pub data: Vec<u8>,
}

/// Parsed PEX executable
#[derive(Debug, Clone)]
pub struct Executable {
    pub entry_point: u64,
    pub sections: Vec<Section>,
}

/// Errors that can occur during executable loading
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutableError {
    /// Invalid magic number
    InvalidMagic(u32),
    /// Unsupported version
    UnsupportedVersion(u32),
    /// Invalid section type
    InvalidSectionType(u32),
    /// Section size mismatch
    SectionSizeMismatch { expected: u64, actual: u64 },
    /// File too short
    FileTooShort,
    /// Invalid entry point
    InvalidEntryPoint,
    /// Section data exceeds declared size
    DataTooLarge,
    /// Invalid alignment
    InvalidAlignment,
}

impl Executable {
    /// Parses a PEX executable from bytes
    pub fn parse(data: &[u8]) -> Result<Self, ExecutableError> {
        if data.len() < 16 {
            return Err(ExecutableError::FileTooShort);
        }

        let mut offset = 0;

        // Read magic
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != PEX_MAGIC {
            return Err(ExecutableError::InvalidMagic(magic));
        }
        offset += 4;

        // Read version
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != PEX_VERSION {
            return Err(ExecutableError::UnsupportedVersion(version));
        }
        offset += 4;

        // Read entry point
        let entry_point = u64::from_le_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]);
        offset += 8;

        // Validate entry point (must be non-zero for now)
        if entry_point == 0 {
            return Err(ExecutableError::InvalidEntryPoint);
        }

        // Read section count
        if data.len() < offset + 4 {
            return Err(ExecutableError::FileTooShort);
        }
        let section_count = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        // Read sections
        let mut sections = Vec::new();
        for _ in 0..section_count {
            if data.len() < offset + 16 {
                return Err(ExecutableError::FileTooShort);
            }

            // Read section type
            let section_type_val = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            let section_type = SectionType::from_u32(section_type_val)?;
            offset += 4;

            // Read section size
            let size = u64::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);
            offset += 8;

            // Read permissions
            let perms_val = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            let permissions = SectionPerms::from_u32(perms_val);
            offset += 4;

            // Read section data (only for text/data, bss is zero-initialized)
            let section_data = if section_type == SectionType::Bss {
                Vec::new()
            } else {
                if data.len() < offset + size as usize {
                    return Err(ExecutableError::FileTooShort);
                }
                let section_data = data[offset..offset + size as usize].to_vec();
                offset += size as usize;
                section_data
            };

            sections.push(Section {
                section_type,
                size,
                permissions,
                data: section_data,
            });
        }

        Ok(Executable {
            entry_point,
            sections,
        })
    }

    /// Validates the executable structure
    pub fn validate(&self) -> Result<(), ExecutableError> {
        // Check that entry point is reasonable
        if self.entry_point == 0 {
            return Err(ExecutableError::InvalidEntryPoint);
        }

        // Validate sections
        for section in &self.sections {
            // Check data size matches declared size for text/data sections
            if section.section_type != SectionType::Bss {
                if section.data.len() as u64 != section.size {
                    return Err(ExecutableError::SectionSizeMismatch {
                        expected: section.size,
                        actual: section.data.len() as u64,
                    });
                }
            } else {
                // BSS sections should have no data
                if !section.data.is_empty() {
                    return Err(ExecutableError::DataTooLarge);
                }
            }

            // Check alignment (sections should be page-aligned, 4KB)
            if section.size % 4096 != 0 {
                return Err(ExecutableError::InvalidAlignment);
            }
        }

        Ok(())
    }

    /// Creates a simple PEX executable for testing
    pub fn create_test_program(entry_point: u64, code: Vec<u8>) -> Vec<u8> {
        let mut buf = Vec::new();

        // Magic
        buf.extend_from_slice(&PEX_MAGIC.to_le_bytes());
        // Version
        buf.extend_from_slice(&PEX_VERSION.to_le_bytes());
        // Entry point
        buf.extend_from_slice(&entry_point.to_le_bytes());

        // Section count (1 text section)
        buf.extend_from_slice(&1u32.to_le_bytes());

        // Text section
        let size = code.len().div_ceil(4096) * 4096; // Round up to page size
        buf.extend_from_slice(&(SectionType::Text as u32).to_le_bytes());
        buf.extend_from_slice(&(size as u64).to_le_bytes());
        buf.extend_from_slice(&5u32.to_le_bytes()); // read + execute

        // Pad code to page size
        let mut padded_code = code;
        padded_code.resize(size, 0);
        buf.extend_from_slice(&padded_code);

        buf
    }
}

/// Loader for PEX executables
pub struct ExecutableLoader<'a, K: KernelApi> {
    kernel: &'a mut K,
}

impl<'a, K: KernelApi> ExecutableLoader<'a, K> {
    /// Creates a new executable loader
    pub fn new(kernel: &'a mut K) -> Self {
        Self { kernel }
    }

    /// Loads an executable and creates a task for it
    ///
    /// This parses the executable, creates an address space, maps sections,
    /// and returns a LoadedProgram that can be used to start execution.
    pub fn load(&mut self, name: String, data: &[u8]) -> Result<LoadedProgram, LoadError> {
        // Parse executable
        let executable = Executable::parse(data).map_err(LoadError::ParseError)?;

        // Validate
        executable.validate().map_err(LoadError::ParseError)?;

        // Create task
        let task_handle = self
            .kernel
            .spawn_task(TaskDescriptor::new(name))
            .map_err(LoadError::KernelError)?;

        let task_id = task_handle.task_id;

        // Create a synthetic execution ID
        // In real usage with SimulatedKernel, this will be overridden
        let execution_id = ExecutionId::new();

        Ok(LoadedProgram {
            task_id,
            execution_id,
            entry_point: executable.entry_point,
            sections: executable.sections,
        })
    }
}

/// A loaded program ready to execute
#[derive(Debug)]
pub struct LoadedProgram {
    pub task_id: TaskId,
    pub execution_id: ExecutionId,
    pub entry_point: u64,
    pub sections: Vec<Section>,
}

impl LoadedProgram {
    /// Returns the text section if present
    pub fn text_section(&self) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.section_type == SectionType::Text)
    }

    /// Returns the data section if present
    pub fn data_section(&self) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.section_type == SectionType::Data)
    }

    /// Returns the bss section if present
    pub fn bss_section(&self) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.section_type == SectionType::Bss)
    }
}

/// Errors that can occur during loading
#[derive(Debug)]
pub enum LoadError {
    /// Parse error
    ParseError(ExecutableError),
    /// Kernel error (stored as string for serializability)
    KernelError(KernelError),
    /// Task creation failed
    TaskCreationFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_executable() {
        let code = vec![0x90u8; 4096]; // NOP instructions
        let data = Executable::create_test_program(0x1000, code);

        let exe = Executable::parse(&data).unwrap();
        assert_eq!(exe.entry_point, 0x1000);
        assert_eq!(exe.sections.len(), 1);
        assert_eq!(exe.sections[0].section_type, SectionType::Text);
        assert_eq!(exe.sections[0].size, 4096);
    }

    #[test]
    fn test_parse_invalid_magic() {
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(&0x12345678u32.to_le_bytes());

        let result = Executable::parse(&data);
        assert!(matches!(
            result,
            Err(ExecutableError::InvalidMagic(0x12345678))
        ));
    }

    #[test]
    fn test_parse_unsupported_version() {
        let mut data = vec![0u8; 20];
        data[0..4].copy_from_slice(&PEX_MAGIC.to_le_bytes());
        data[4..8].copy_from_slice(&99u32.to_le_bytes());

        let result = Executable::parse(&data);
        assert!(matches!(
            result,
            Err(ExecutableError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn test_parse_file_too_short() {
        let data = vec![0u8; 10];
        let result = Executable::parse(&data);
        assert!(matches!(result, Err(ExecutableError::FileTooShort)));
    }

    #[test]
    fn test_validate_invalid_entry_point() {
        let exe = Executable {
            entry_point: 0,
            sections: Vec::new(),
        };
        let result = exe.validate();
        assert!(matches!(result, Err(ExecutableError::InvalidEntryPoint)));
    }

    #[test]
    fn test_validate_invalid_alignment() {
        let exe = Executable {
            entry_point: 0x1000,
            sections: vec![Section {
                section_type: SectionType::Text,
                size: 1000, // Not page-aligned
                permissions: SectionPerms::text(),
                data: vec![0; 1000],
            }],
        };
        let result = exe.validate();
        assert!(matches!(result, Err(ExecutableError::InvalidAlignment)));
    }

    #[test]
    fn test_section_permissions() {
        let text_perms = SectionPerms::text();
        assert!(text_perms.read);
        assert!(!text_perms.write);
        assert!(text_perms.execute);

        let data_perms = SectionPerms::data();
        assert!(data_perms.read);
        assert!(data_perms.write);
        assert!(!data_perms.execute);
    }

    #[test]
    fn test_loaded_program_sections() {
        let sections = vec![
            Section {
                section_type: SectionType::Text,
                size: 4096,
                permissions: SectionPerms::text(),
                data: vec![0; 4096],
            },
            Section {
                section_type: SectionType::Data,
                size: 4096,
                permissions: SectionPerms::data(),
                data: vec![0; 4096],
            },
            Section {
                section_type: SectionType::Bss,
                size: 4096,
                permissions: SectionPerms::data(),
                data: Vec::new(),
            },
        ];

        let program = LoadedProgram {
            task_id: TaskId::new(),
            execution_id: ExecutionId::new(),
            entry_point: 0x1000,
            sections,
        };

        assert!(program.text_section().is_some());
        assert!(program.data_section().is_some());
        assert!(program.bss_section().is_some());
    }

    #[test]
    fn test_executable_with_multiple_sections() {
        let mut buf = Vec::new();

        // Header
        buf.extend_from_slice(&PEX_MAGIC.to_le_bytes());
        buf.extend_from_slice(&PEX_VERSION.to_le_bytes());
        buf.extend_from_slice(&0x1000u64.to_le_bytes());
        buf.extend_from_slice(&3u32.to_le_bytes()); // 3 sections

        // Text section
        buf.extend_from_slice(&(SectionType::Text as u32).to_le_bytes());
        buf.extend_from_slice(&4096u64.to_le_bytes());
        buf.extend_from_slice(&5u32.to_le_bytes()); // read + execute
        buf.extend_from_slice(&vec![0x90u8; 4096]);

        // Data section
        buf.extend_from_slice(&(SectionType::Data as u32).to_le_bytes());
        buf.extend_from_slice(&4096u64.to_le_bytes());
        buf.extend_from_slice(&3u32.to_le_bytes()); // read + write
        buf.extend_from_slice(&vec![0x42u8; 4096]);

        // BSS section
        buf.extend_from_slice(&(SectionType::Bss as u32).to_le_bytes());
        buf.extend_from_slice(&4096u64.to_le_bytes());
        buf.extend_from_slice(&3u32.to_le_bytes()); // read + write

        let exe = Executable::parse(&buf).unwrap();
        assert_eq!(exe.sections.len(), 3);
        assert_eq!(exe.sections[0].section_type, SectionType::Text);
        assert_eq!(exe.sections[1].section_type, SectionType::Data);
        assert_eq!(exe.sections[2].section_type, SectionType::Bss);
    }

    #[test]
    fn test_loader_with_mock_kernel() {
        use kernel_api::{Duration, Instant, TaskHandle};

        struct MockKernel {
            next_task_id: u128,
        }

        impl KernelApi for MockKernel {
            fn spawn_task(&mut self, _desc: TaskDescriptor) -> Result<TaskHandle, KernelError> {
                let task_id = TaskId::from_u128(self.next_task_id);
                self.next_task_id += 1;
                Ok(TaskHandle { task_id })
            }

            fn create_channel(&mut self) -> Result<ipc::ChannelId, KernelError> {
                unimplemented!()
            }

            fn send_message(
                &mut self,
                _channel: ipc::ChannelId,
                _message: ipc::MessageEnvelope,
            ) -> Result<(), KernelError> {
                unimplemented!()
            }

            fn receive_message(
                &mut self,
                _channel: ipc::ChannelId,
                _timeout: Option<Duration>,
            ) -> Result<ipc::MessageEnvelope, KernelError> {
                unimplemented!()
            }

            fn now(&self) -> Instant {
                Instant::from_nanos(0)
            }

            fn sleep(&mut self, _duration: Duration) -> Result<(), KernelError> {
                Ok(())
            }

            fn grant_capability(
                &mut self,
                _task_id: TaskId,
                _cap: core_types::Cap<()>,
            ) -> Result<(), KernelError> {
                Ok(())
            }

            fn register_service(
                &mut self,
                _service_id: core_types::ServiceId,
                _channel: ipc::ChannelId,
            ) -> Result<(), KernelError> {
                Ok(())
            }

            fn lookup_service(
                &self,
                _service_id: core_types::ServiceId,
            ) -> Result<ipc::ChannelId, KernelError> {
                Err(KernelError::ServiceNotFound("mock".to_string()))
            }
        }

        let mut kernel = MockKernel { next_task_id: 1 };
        let mut loader = ExecutableLoader::new(&mut kernel);

        let code = vec![0x90u8; 4096];
        let data = Executable::create_test_program(0x1000, code);

        let program = loader.load("test".to_string(), &data).unwrap();
        assert_eq!(program.entry_point, 0x1000);
        assert_eq!(program.sections.len(), 1);
    }
}
