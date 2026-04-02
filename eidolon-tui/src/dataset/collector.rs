use serde::Serialize;
use std::fs::{create_dir_all, OpenOptions};
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
                JsonlMessage {
                    role: "system".to_string(),
                    content: example.system_prompt,
                },
                JsonlMessage {
                    role: "user".to_string(),
                    content: example.user_message,
                },
                JsonlMessage {
                    role: "assistant".to_string(),
                    content: example.assistant_response,
                },
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

        if let Some(parent) = self.path.parent() {
            create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let writer = BufWriter::new(file);
        self.flush_into_writer(writer)
    }

    fn flush_into_writer<W: Write>(
        &mut self,
        mut writer: W,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let mut pending = Vec::new();
        std::mem::swap(&mut pending, &mut self.buffer);
        let mut committed = 0;

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            for line in &pending {
                writeln!(writer, "{}", line)?;
                committed += 1;
            }
            writer.flush()?;
            Ok(())
        })();

        if result.is_err() {
            // If all lines were written but flush() failed, don't restore them --
            // they're in the BufWriter's internal buffer and will be flushed next time.
            // Only restore uncommitted lines.
            if committed < pending.len() {
                self.buffer = pending.split_off(committed);
            }
        }

        result
    }

    #[cfg(test)]
    fn flush_with_custom_writer<W: Write>(
        &mut self,
        writer: W,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.flush_into_writer(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, Write};

    struct FlushFailWriter {
        buffer: Vec<u8>,
    }

    impl FlushFailWriter {
        fn new() -> Self {
            Self { buffer: Vec::new() }
        }
    }

    impl Write for FlushFailWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(Error::other("simulated flush failure"))
        }
    }

    #[test]
    fn flush_preserves_buffer_on_failure() {
        let mut collector = DatasetCollector::new(PathBuf::from("training.jsonl"));

        for i in 0..2 {
            collector
                .record(TrainingExample {
                    system_prompt: "System".to_string(),
                    user_message: format!("Message {}", i),
                    assistant_response: format!("Response {}", i),
                    intent: "casual".to_string(),
                    tools_called: vec![],
                    user_override: false,
                })
                .unwrap();
        }

        let writer = FlushFailWriter::new();
        assert!(collector.flush_with_custom_writer(writer).is_err());
        // All lines were written successfully before flush() failed,
        // so they should NOT be restored (they're in the writer's buffer).
        assert_eq!(collector.buffer.len(), 0);
    }
}
