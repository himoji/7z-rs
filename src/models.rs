pub struct ArchiveZone {
    pub rect: Option<egui::Rect>,
    pub is_compress_zone: bool,
}

impl Default for ArchiveZone {
    fn default() -> Self {
        Self {
            rect: None,
            is_compress_zone: false,
        }
    }
}

#[derive(Default)]
pub struct Password(pub Option<String>);

impl Password {
    pub fn set(&mut self, value: String) {
        self.0 = if value.is_empty() { None } else { Some(value) };
    }

    pub fn as_str(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

pub struct ArchiveFile {
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
}
