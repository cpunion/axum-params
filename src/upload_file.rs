use serde::{Deserialize, Serialize};
use tokio::fs::File;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UploadFile {
    pub name: String,
    pub content_type: String,
    pub(crate) temp_file_path: String,
}

impl PartialEq for UploadFile {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.content_type == other.content_type
    }
}

impl UploadFile {
    pub fn open(&self) -> impl std::future::Future<Output = Result<File, std::io::Error>> + '_ {
        File::open(&self.temp_file_path)
    }
}
