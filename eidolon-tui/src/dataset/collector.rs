use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct TrainingExample {
    pub system_prompt: String,
    pub user_message: String,
    pub assistant_response: String,
    pub intent: String,
    pub tools_called: Vec<String>,
    pub user_override: bool,
}

#[derive(Serialize)]
struct JsonlEntry {
    messages: Vec<JsonlMessage>,
    metadata: JsonlMetadata,
}

#[derive(Serialize)]
struct JsonlMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct JsonlMetadata {
    intent: String,
    tools_called: Vec<String>,
    user_override: bool,
    timestamp: String,
}

pub struct DatasetCollector {
    path: PathBuf,
    buffer: Vec<String>,
}

impl DatasetCollector {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            buffer: Vec::new(),
        }
    }

    pub fn record(&mut self, example: TrainingExample) -> Result<(), Box<dyn std::error::Error>> {
        let entry = JsonlEntry {
            messages: vec![
                JsonlMessage { role: "system".to_string(), content: example.system_prompt },
                JsonlMessage { role: "user".to_string(), content: example.user_message },
                JsonlMessage { role: "assistant".to_string(), content: example.assistant_response },
            ],
            metadata: JsonlMetadata {
                intent: example.intent,
                tools_called: example.tools_called,
                user_override: example.user_override,
                timestamp: chrono::Utc::now().to_rfc3339(),
            },
        };

        let line = serde_json::to_string(&entry)?;
        self.buffer.push(line);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let mut writer = BufWriter::new(file);
        for line in self.buffer.drain(..) {
            writeln!(writer, "{}", line)?;
        }
        writer.flush()?;
        Ok(())
    }
}
